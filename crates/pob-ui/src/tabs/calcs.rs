//! `Classes/CalcsTab.lua` (the tab; the power builder is `tabs/tree.rs` over R8.7) and `Classes/CalcSectionControl.lua`. The model of the sections, the texts and
//! the visibility are `pob_engine::calcs`, the output is the engine's CALCS run: the tab does not recalculate by itself, `buildFlag` does (Build.lua:1258).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_build::InputValue;
use pob_engine::calcs::{CalcsActor, CheckCtx, Flags, Section, check_flag, format_calc_str, minion_actor, player_actor, sections};
use pob_engine::calcs_tab::BdEnv;
use pob_engine::display::DisplaySettings;
use pob_engine::shell::MinionEntry;
use pob_gfx::{Frame, Gfx};

use super::calc_breakdown::{CalcBreakdownControl, Source};
use crate::colors::color_code;
use crate::control::{Control, ControlBase, ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::modules::build::{BuildMode, SkillControls, apply_skill_select};
use crate::modules::main::{BareControl, main, not_ported, sel_by_value};
use crate::sg;
use crate::undo::{UndoHandler, UndoHost};

/// `colData`: `(section, subsection, row, cell)` (0-based).
pub type CellKey = (usize, usize, usize, usize);

const BUFF_MODES: [(&str, &str); 4] = [
    ("Unbuffed", "UNBUFFED"),
    ("Buffed", "BUFFED"),
    ("In Combat", "COMBAT"),
    ("Effective DPS", "EFFECTIVE"),
];

const MODE_TOOLTIP: &str = "This controls the calculation of the stats shown in this tab.
The stats in the sidebar are always shown in Effective DPS mode, regardless of this setting.

Unbuffed: No auras, buffs, or other support skills or effects will apply. This is equivalent to standing in town.
Buffed: Aura and buff skills apply. This is equivalent to standing in your hideout with auras and buffs turned on.
In Combat: Charges and combat buffs such as Onslaught will also apply. This will show your character sheet stats in combat.
Effective DPS: Curses and enemy properties (such as resistances and status conditions) will also apply. This estimates your true DPS.";

/// The geometry fields of `colData` (CalcSectionControl.lua:135-138, :175-176).
#[derive(Default)]
struct CellState {
    x_offset: Cell<f64>,
    y_offset: Cell<f64>,
    width: Cell<f64>,
    height: Cell<f64>,
    x: Cell<f64>,
    y: Cell<f64>,
    control: Option<ControlRef>,
}

struct RowState {
    enabled: Cell<bool>,
    cells: Vec<CellState>,
}

struct SubState {
    id: String,
    collapsed: Cell<bool>,
    rows: Vec<RowState>,
}

/// `CalcSectionControl`.
pub struct CalcSection {
    pub base: ControlBase,
    pub host: ControlHost,
    me: Weak<Self>,
    tab: Weak<CalcsTab>,
    pub index: usize,
    def: &'static Section,
    flags: Flags,
    pub enabled: Cell<bool>,
    subs: Vec<SubState>,
    pub is_overlay: Cell<bool>,
    pub overlay_x: Cell<f64>,
    pub overlay_y: Cell<f64>,
    dragging: Cell<bool>,
    drag_off: Cell<(f64, f64)>,
    overlay_revision: Cell<Option<u64>>,
    overlay_breakdown_cell: Cell<bool>,
    has_controls: bool,
}

impl CalcSection {
    fn new(
        tab: Weak<CalcsTab>,
        index: usize,
        def: &'static Section,
        named: &IndexMap<String, ControlRef>,
        collapsed: &dyn Fn(&str, &str) -> Option<bool>,
    ) -> Rc<Self> {
        let first = &def.sub_sections[0];
        let flags = Flags {
            flag: first.flag.clone(),
            not_flag: first.not_flag.clone(),
            ..Flags::default()
        };
        Rc::new_cyclic(|me: &Weak<Self>| {
            let host = ControlHost::new();
            let mut has_controls = false;
            let mut subs = Vec::with_capacity(def.sub_sections.len());
            for (i, sub) in def.sub_sections.iter().enumerate() {
                let id: String = sub.label.chars().filter(char::is_ascii_alphanumeric).collect();
                let mut rows = Vec::with_capacity(sub.rows.len());
                for (j, row) in sub.rows.iter().enumerate() {
                    let mut cells = Vec::with_capacity(row.cells.len());
                    for cell in &row.cells {
                        let mut st = CellState::default();
                        if let Some(name) = &cell.control_name {
                            let c = named.get(name).unwrap_or_else(|| panic!("calcs: control {name}")).clone();
                            has_controls = true;
                            host.add(name, c.clone());
                            let w = me.clone();
                            c.base().shown.set_fn(move |_| {
                                let s = w.upgrade().expect("section");
                                s.enabled.get()
                                    && !s.subs[0].collapsed.get()
                                    && !s.subs[i].collapsed.get()
                                    && s.subs[i].rows[j].enabled.get()
                            });
                            st.control = Some(c);
                        }
                        cells.push(st);
                    }
                    rows.push(RowState {
                        enabled: Cell::new(false),
                        cells,
                    });
                }
                let c = collapsed(&def.id, &id).unwrap_or(sub.default_collapsed);
                subs.push(SubState {
                    id: id.clone(),
                    collapsed: Cell::new(c),
                    rows,
                });
                let w: Weak<CalcSection> = me.clone();
                let wc: crate::control::WeakControl = me.clone();
                let w2 = w.clone();
                let toggle = ButtonControl::new(
                    crate::control::anchor_weak("TOPRIGHT", wc, "TOPRIGHT"),
                    rect(-3.0, -13.0 + 16.0 * (i as f64 + 1.0), 16.0, 16.0),
                    crate::control::Prop::func(move |_| {
                        if w.upgrade().expect("section").subs[i].collapsed.get() {
                            "+"
                        } else {
                            "-"
                        }
                        .to_string()
                    }),
                    move |g| w2.upgrade().expect("section").toggle_sub(g, i),
                );
                let w = me.clone();
                if i == 0 {
                    toggle.base.shown.set_fn(move |_| w.upgrade().expect("section").enabled.get());
                } else {
                    toggle.base.shown.set_fn(move |_| {
                        let s = w.upgrade().expect("section");
                        s.enabled.get() && !s.subs[0].collapsed.get()
                    });
                }
                host.add(&format!("toggle{}", i + 1), toggle);
            }
            let wc: crate::control::WeakControl = me.clone();
            let w = me.clone();
            let pop_out = ButtonControl::new(
                crate::control::anchor_weak("TOPRIGHT", wc, "TOPRIGHT"),
                rect(-22.0, 3.0, 16.0, 16.0),
                "^".to_string(),
                move |g| w.upgrade().expect("section").toggle_overlay(g),
            );
            let w = me.clone();
            pop_out.base.shown.set_fn(move |_| {
                let s = w.upgrade().expect("section");
                s.enabled.get() && !s.is_overlay.get() && !s.has_controls
            });
            host.add("popOut", pop_out);
            let base = ControlBase::new(None, rect(0.0, 0.0, def.width(), 0.0));
            let w = me.clone();
            base.shown.set_fn(move |_| {
                let s = w.upgrade().expect("section");
                s.enabled.get() && !s.is_overlay.get()
            });
            Self {
                base,
                host,
                me: me.clone(),
                tab,
                index,
                def,
                flags,
                enabled: Cell::new(false),
                subs,
                is_overlay: Cell::new(false),
                overlay_x: Cell::new(320.0),
                overlay_y: Cell::new(50.0),
                dragging: Cell::new(false),
                drag_off: Cell::new((0.0, 0.0)),
                overlay_revision: Cell::new(None),
                overlay_breakdown_cell: Cell::new(false),
                has_controls,
            }
        })
    }

    fn tab(&self) -> Rc<CalcsTab> {
        self.tab.upgrade().expect("calcsTab")
    }

    /// The toggle's onClick (:52-55) and the overlay's (:318-320).
    fn toggle_sub(&self, _g: &mut Gfx, i: usize) {
        let s = &self.subs[i];
        s.collapsed.set(!s.collapsed.get());
        let tab = self.tab();
        tab.undo.mod_flag.set(true);
        let b = tab.build();
        b.engine
            .borrow_mut()
            .as_mut()
            .expect("engine")
            .set_calcs_collapsed(&self.def.id, &s.id, s.collapsed.get());
    }

    /// :84-112 `IsMouseOver` with its second result.
    fn mouse_over_comp(&self, g: &Gfx) -> (bool, Option<CellKey>) {
        if !self.is_shown(g) {
            return (false, None);
        }
        if self.host.get_mouse_over_control(g).is_some() {
            return (true, None);
        }
        let m_over = self.is_mouse_in_bounds(g);
        if !(m_over && self.enabled.get()) {
            return (m_over, None);
        }
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        for (i, sub) in self.subs.iter().enumerate() {
            if sub.collapsed.get() {
                continue;
            }
            for (j, row) in sub.rows.iter().enumerate() {
                if !row.enabled.get() {
                    continue;
                }
                for (k, c) in row.cells.iter().enumerate() {
                    if let Some(ctl) = &c.control {
                        if ctl.is_mouse_over(g) == Some(true) {
                            return (m_over, Some((self.index, i, j, k)));
                        }
                    } else if cursor_x >= c.x.get()
                        && cursor_y >= c.y.get()
                        && cursor_x < c.x.get() + c.width.get()
                        && cursor_y < c.y.get() + c.height.get()
                    {
                        return (m_over, Some((self.index, i, j, k)));
                    }
                }
            }
        }
        (m_over, None)
    }

    /// :114-163 `UpdateSize`.
    pub fn update_size(&self, g: &mut Gfx) {
        let tab = self.tab();
        let enabled = tab.with_actor(|ctx| check_flag(ctx, &self.flags)).unwrap_or_else(|e| panic!("{e}"));
        self.enabled.set(enabled);
        if !enabled {
            self.base.height.set(22.0);
            return;
        }
        let width = self.get_size(g).0;
        let mut height = 2.0;
        self.enabled.set(false);
        let mut y_offset = 0.0;
        tab.with_actor(|ctx| {
            for (i, sub) in self.def.sub_sections.iter().enumerate() {
                let st = &self.subs[i];
                let toggle = self.host.get(&format!("toggle{}", i + 1)).expect("toggle");
                toggle.base().y.set(y_offset + 3.0);
                let mut temp = 0.0;
                y_offset += 22.0;
                for (j, row) in sub.rows.iter().enumerate() {
                    let rs = &st.rows[j];
                    let e = check_flag(ctx, &row.flags).unwrap_or_else(|e| panic!("{e}"));
                    rs.enabled.set(e);
                    if e {
                        self.enabled.set(true);
                        let mut x_offset = 134.0;
                        for c in &rs.cells {
                            c.x_offset.set(x_offset);
                            c.y_offset.set(y_offset);
                            c.width.set(sub.col_width.unwrap_or(width - 136.0));
                            c.height.set(18.0);
                            x_offset += c.width.get();
                        }
                        y_offset += 18.0;
                        height += 18.0;
                        temp += 18.0;
                    }
                    if st.collapsed.get() {
                        rs.enabled.set(false);
                    }
                }
                if self.enabled.get() && !st.collapsed.get() {
                    height += 22.0;
                } else {
                    height = height - temp + 20.0;
                    y_offset = y_offset - temp - 2.0;
                }
            }
        });
        self.base.height.set(height);
        if self.enabled.get() && !self.subs[0].collapsed.get() {
            if self.def.has_update_func {
                tab.update_skill_select(g);
            }
        } else {
            self.base.height.set(22.0);
        }
    }

    /// :165-185 `UpdatePos`.
    pub fn update_pos(&self, g: &Gfx) {
        if !self.enabled.get() {
            return;
        }
        let (x, y) = self.get_pos(g);
        for sub in &self.subs {
            for row in &sub.rows {
                if row.enabled.get() {
                    for c in &row.cells {
                        c.x.set(x + c.x_offset.get());
                        c.y.set(y + c.y_offset.get());
                        if let Some(ctl) = &c.control {
                            ctl.base().x.set(c.x.get() + 4.0);
                            ctl.base().y.set(c.y.get() + 9.0 - ctl.base().height.get(g) / 2.0);
                        }
                    }
                }
            }
        }
    }

    /// :234-251 `ToggleOverlay`.
    pub fn toggle_overlay(&self, g: &mut Gfx) {
        self.is_overlay.set(!self.is_overlay.get());
        let b = self.tab().build();
        let me = self.me.upgrade().expect("section");
        if self.is_overlay.get() {
            let (x, y) = self.get_pos(g);
            self.overlay_x.set(x);
            self.overlay_y.set(y);
            b.overlay_panes.borrow_mut().push(me);
        } else {
            b.overlay_panes.borrow_mut().retain(|p| !Rc::ptr_eq(p, &me));
            self.dragging.set(false);
        }
    }

    /// :253-262
    fn raise_overlay(&self) {
        let me = self.me.upgrade().expect("section");
        let b = self.tab().build();
        let mut panes = b.overlay_panes.borrow_mut();
        if let Some(i) = panes.iter().position(|p| Rc::ptr_eq(p, &me)) {
            let p = panes.remove(i);
            panes.push(p);
        }
    }

    /// :264-270
    pub fn is_mouse_in_overlay(&self, g: &Gfx, cursor_x: f64, cursor_y: f64) -> bool {
        if !self.is_overlay.get() {
            return false;
        }
        let (x, y) = (self.overlay_x.get(), self.overlay_y.get());
        let width = self.get_size(g).0;
        if cursor_x < x || cursor_x > x + width || cursor_y < y {
            return false;
        }
        cursor_y < y + self.overlay_height()
    }

    fn row_check(&self, row: &pob_engine::calcs::Row) -> bool {
        self.tab()
            .with_actor(|ctx| check_flag(ctx, &row.flags))
            .unwrap_or_else(|e| panic!("{e}"))
    }

    fn section_check(&self) -> bool {
        self.tab()
            .with_actor(|ctx| check_flag(ctx, &self.flags))
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// :272-289
    fn overlay_height(&self) -> f64 {
        let mut height = 28.0;
        let enabled = self.section_check();
        for (i, sub) in self.def.sub_sections.iter().enumerate() {
            height += 22.0;
            if !self.subs[i].collapsed.get() && enabled {
                for row in &sub.rows {
                    if self.row_check(row) {
                        height += 18.0;
                    }
                }
                height += 2.0;
            } else if i == 0 {
                break;
            }
        }
        height
    }

    /// :291-360
    pub fn handle_overlay_click(&self, g: &mut Gfx, key: &str, cursor_x: f64, cursor_y: f64) {
        if key != "LEFTBUTTON" {
            return;
        }
        self.raise_overlay();
        let (x, y) = (self.overlay_x.get(), self.overlay_y.get());
        let overlay_width = self.get_size(g).0;
        let close_x = x + overlay_width - 18.0;
        let close_y = y + 2.0;
        if cursor_x >= close_x && cursor_x <= close_x + 14.0 && cursor_y >= close_y && cursor_y <= close_y + 16.0 {
            self.toggle_overlay(g);
            return;
        }
        let mut line_y = y + 26.0;
        let mut primary = true;
        for (i, sub) in self.def.sub_sections.iter().enumerate() {
            let sec_enabled = self.section_check();
            if sec_enabled {
                let toggle_x = x + overlay_width - 18.0;
                let toggle_y = line_y + 3.0;
                if cursor_x >= toggle_x && cursor_x <= toggle_x + 14.0 && cursor_y >= toggle_y && cursor_y <= toggle_y + 16.0 {
                    self.toggle_sub(g, i);
                    self.overlay_revision.set(None);
                    return;
                }
            }
            if self.subs[i].collapsed.get() || !sec_enabled {
                if primary {
                    break;
                }
                line_y += 20.0;
                primary = false;
            } else {
                line_y += 20.0;
                primary = false;
                for (j, row) in sub.rows.iter().enumerate() {
                    if self.row_check(row) {
                        for (k, c) in self.subs[i].rows[j].cells.iter().enumerate() {
                            let cell_x = x + if c.x_offset.get() != 0.0 { c.x_offset.get() } else { 134.0 };
                            let cell_w = if c.width.get() != 0.0 {
                                c.width.get()
                            } else {
                                overlay_width - 136.0
                            };
                            if cursor_x >= cell_x && cursor_x <= cell_x + cell_w && cursor_y >= line_y + 2.0 && cursor_y <= line_y + 19.0 {
                                let def = &row.cells[k];
                                if def.format.is_some()
                                    && self
                                        .tab()
                                        .with_actor(|ctx| check_flag(ctx, &def.flags))
                                        .unwrap_or_else(|e| panic!("{e}"))
                                {
                                    self.tab().set_display_stat(g, (self.index, i, j, k), true);
                                }
                                return;
                            }
                        }
                        line_y += 18.0;
                    }
                }
                line_y += 2.0;
            }
        }
        if cursor_y >= y && cursor_y <= y + 24.0 {
            self.dragging.set(true);
            self.drag_off.set((cursor_x - x, cursor_y - y));
        }
    }

    /// :362-366
    pub fn handle_overlay_release(&self, key: &str) {
        if key == "LEFTBUTTON" {
            self.dragging.set(false);
        }
    }

    /// :368-435 `DrawOverlay`.
    pub fn draw_overlay(&self, f: &mut Frame<'_>, view_port: &Rect) {
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        self.overlay_breakdown_cell.set(false);
        let tab = self.tab();
        let rev = tab.build().engine.borrow().as_ref().expect("engine").output_revision();
        if self.overlay_revision.get() != Some(rev) {
            self.update_size(f);
            self.overlay_revision.set(Some(rev));
        }
        if self.dragging.get() {
            let (dx, dy) = self.drag_off.get();
            self.overlay_x.set(cursor_x - dx);
            self.overlay_y.set(cursor_y - dy);
        }
        let overlay_width = self.get_size(f).0;
        self.overlay_x.set(
            view_port
                .x
                .max(self.overlay_x.get().min(view_port.x + view_port.width - overlay_width)),
        );
        self.overlay_y
            .set(view_port.y.max(self.overlay_y.get().min(view_port.y + view_port.height - 24.0)));
        let (x, y) = (self.overlay_x.get(), self.overlay_y.get());
        let total_height = self.overlay_height();
        sg::set_draw_layer(f, Some(10), 0);
        sg::set_draw_color_a(f, 0.08, 0.08, 0.08, 0.95);
        sg::draw_rect(f, x, y, overlay_width, total_height);
        sg::set_draw_color_str(f, &self.def.colour);
        sg::draw_rect(f, x, y, overlay_width, 1.0);
        sg::draw_rect(f, x, y + total_height - 1.0, overlay_width, 1.0);
        sg::draw_rect(f, x, y, 1.0, total_height);
        sg::draw_rect(f, x + overlay_width - 1.0, y, 1.0, total_height);
        sg::set_draw_color(f, 0.18, 0.18, 0.18);
        sg::draw_rect(f, x + 1.0, y + 1.0, overlay_width - 2.0, 22.0);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        let (cbx, cby) = (x + overlay_width - 16.0, y + 3.0);
        sg::draw_quad(
            f,
            [
                cbx + 3.0,
                cby + 5.0,
                cbx + 5.0,
                cby + 3.0,
                cbx + 13.0,
                cby + 11.0,
                cbx + 11.0,
                cby + 13.0,
            ],
        );
        sg::draw_quad(
            f,
            [
                cbx + 11.0,
                cby + 3.0,
                cbx + 13.0,
                cby + 5.0,
                cbx + 5.0,
                cby + 13.0,
                cbx + 3.0,
                cby + 11.0,
            ],
        );
        sg::set_draw_color_str(f, &self.def.colour);
        sg::draw_rect(f, x + 2.0, y + 24.0, overlay_width - 4.0, 1.0);
        self.draw_content(f, x, y + 26.0, overlay_width, view_port, true, false);
        let display = *tab.display_data.borrow();
        if let Some(k) = display
            && (self.overlay_breakdown_cell.get() || (tab.display_pinned.get() && k.0 == self.index))
        {
            let c = &tab.sections[k.0].subs[k.1].rows[k.2].cells[k.3];
            let (ox, oy) = (c.x.get(), c.y.get());
            let or = |v: f64, d: f64| if v != 0.0 { v } else { d };
            c.x.set(x + or(c.x_offset.get(), 134.0));
            c.y.set(y + 26.0 + c.y_offset.get());
            tab.breakdown.draw_impl(f, view_port);
            c.x.set(ox);
            c.y.set(oy);
        }
        self.overlay_breakdown_cell.set(false);
        sg::set_draw_layer(f, Some(0), 0);
    }

    /// :437-551 `DrawContent`.
    #[allow(clippy::too_many_arguments)]
    fn draw_content(
        &self,
        f: &mut Frame<'_>,
        draw_x: f64,
        start_line_y: f64,
        draw_width: f64,
        view_port: &Rect,
        is_overlay: bool,
        no_tooltip: bool,
    ) {
        let tab = self.tab();
        let st = BuildMode::display_settings();
        tab.with_actor(|ctx| {
            self.draw_content_with(
                f,
                ctx,
                &tab,
                &st,
                (draw_x, start_line_y, draw_width),
                view_port,
                is_overlay,
                no_tooltip,
            )
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_content_with(
        &self,
        f: &mut Frame<'_>,
        ctx: &CheckCtx<'_, '_>,
        tab: &Rc<CalcsTab>,
        st: &DisplaySettings,
        at: (f64, f64, f64),
        view_port: &Rect,
        is_overlay: bool,
        no_tooltip: bool,
    ) {
        let (draw_x, start_line_y, draw_width) = at;
        let actor: &CalcsActor<'_> = ctx.actor;
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        let mut line_y = start_line_y;
        let mut primary = true;
        let chk = |fl: &Flags| check_flag(ctx, fl).unwrap_or_else(|e| panic!("{e}"));
        let enabled = if is_overlay { chk(&self.flags) } else { self.enabled.get() };
        let highlight = color_code("HIGHLIGHT").expect("HIGHLIGHT");
        let highlight = highlight.as_str();
        let fmt = |s: &str, cell: Option<&pob_engine::calcs::Cell>| {
            format_calc_str(s, actor, cell, st).unwrap_or_else(|e| panic!("Modules/CalcFormat.lua: {e}"))
        };
        for (i, sub) in self.def.sub_sections.iter().enumerate() {
            let ss = &self.subs[i];
            sg::set_draw_color_str(f, &self.def.colour);
            sg::draw_rect(f, draw_x + 2.0, line_y, draw_width - 4.0, 2.0);
            let lx = if is_overlay { draw_x + 4.0 } else { draw_x + 3.0 };
            if !enabled {
                sg::draw_string(f, lx, line_y + 3.0, Some("LEFT"), 16.0, "VAR BOLD", &format!("^8{}", sub.label));
            } else {
                let text_color = if !is_overlay && tab.search_match(&sub.label) {
                    highlight
                } else {
                    "^7"
                };
                sg::draw_string(
                    f,
                    lx,
                    line_y + 3.0,
                    Some("LEFT"),
                    16.0,
                    "VAR BOLD",
                    &format!("{text_color}{}:", sub.label),
                );
                if let Some(extra) = &sub.extra {
                    let ex = lx + sg::draw_string_width(f, 16.0, "VAR BOLD", &sub.label) + if is_overlay { 12.0 } else { 10.0 };
                    sg::draw_string(f, ex, line_y + 3.0, Some("LEFT"), 16.0, "VAR", &format!("^7{}", fmt(extra, None)));
                }
            }
            sg::set_draw_color_str(f, &self.def.colour);
            sg::draw_rect(f, draw_x + 2.0, line_y + 20.0, draw_width - 4.0, 2.0);
            if is_overlay {
                if enabled {
                    let t = if ss.collapsed.get() { "^7+" } else { "^7-" };
                    sg::draw_string(f, draw_x + draw_width - 16.0, line_y + 3.0, Some("LEFT"), 14.0, "VAR", t);
                }
            } else {
                sg::set_draw_layer(f, None, 0);
                let sel = if no_tooltip { tab.host.sel() } else { None };
                self.host.draw_controls(f, view_port, sel.as_ref());
            }
            if ss.collapsed.get() || !enabled {
                if primary {
                    return;
                }
                line_y += 20.0;
                primary = false;
            } else {
                line_y += 20.0;
                primary = false;
                let mut rows = 0;
                for (j, row) in sub.rows.iter().enumerate() {
                    let rs = &ss.rows[j];
                    if !((is_overlay && chk(&row.flags)) || (!is_overlay && rs.enabled.get())) {
                        continue;
                    }
                    rows += 1;
                    let bg = row.bg_col.as_deref().unwrap_or("^0");
                    if !is_overlay {
                        if let Some(label) = &row.label {
                            let mut text_color = row.color.as_deref().unwrap_or("^7");
                            sg::set_draw_color_str(f, bg);
                            sg::draw_rect(f, draw_x + 2.0, line_y + 2.0, 130.0, 18.0);
                            if tab.search_match(label) {
                                text_color = highlight;
                            }
                            sg::draw_string(
                                f,
                                draw_x + 132.0,
                                line_y + 2.0,
                                Some("RIGHT_X"),
                                16.0,
                                "VAR",
                                &format!("{text_color}{label}^7:"),
                            );
                        }
                    } else if let Some(label) = &row.label {
                        sg::draw_string(
                            f,
                            draw_x + 132.0,
                            line_y + 2.0,
                            Some("RIGHT_X"),
                            16.0,
                            "VAR",
                            &format!("^7{label}^7:"),
                        );
                    }
                    for (k, cell) in row.cells.iter().enumerate() {
                        let c = &rs.cells[k];
                        let or = |v: f64, d: f64| if v != 0.0 { v } else { d };
                        let cell_x = if is_overlay {
                            draw_x + or(c.x_offset.get(), 134.0)
                        } else {
                            c.x.get()
                        };
                        let cell_w = if is_overlay {
                            or(c.width.get(), draw_width - 136.0)
                        } else {
                            c.width.get()
                        };
                        let cell_y = if is_overlay { line_y + 2.0 } else { c.y.get() };
                        let cell_h = or(c.height.get(), 18.0);
                        sg::set_draw_color_str(f, &self.def.colour);
                        sg::draw_rect(f, cell_x, line_y + 2.0, 2.0, cell_h);
                        let Some(format) = &cell.format else { continue };
                        // `isOverlay or CheckFlag(colData)`, then `isOverlay and actor and CheckFlag(colData) or not isOverlay`
                        if !chk(&cell.flags) {
                            continue;
                        }
                        let key = (self.index, i, j, k);
                        if is_overlay {
                            if cursor_y >= line_y + 2.0 && cursor_y < line_y + 20.0 && cursor_x >= cell_x && cursor_x < cell_x + cell_w {
                                tab.set_display_hover(f, key);
                                self.overlay_breakdown_cell.set(true);
                            }
                        } else if cursor_y >= view_port.y
                            && cursor_y < view_port.y + view_port.height
                            && cursor_x >= cell_x
                            && cursor_y >= cell_y
                            && cursor_x < cell_x + cell_w
                            && cursor_y < cell_y + cell_h
                        {
                            tab.set_display_hover(f, key);
                        }
                        if !is_overlay && *tab.display_data.borrow() == Some(key) {
                            sg::set_draw_color(f, 0.25, 1.0, 0.25);
                            sg::draw_rect(f, cell_x + 2.0, cell_y, cell_w - 2.0, cell_h);
                            sg::set_draw_color_str(f, bg);
                            sg::draw_rect(f, cell_x + 3.0, cell_y + 1.0, cell_w - 4.0, cell_h - 2.0);
                        } else {
                            sg::set_draw_color_str(f, bg);
                            sg::draw_rect(f, cell_x + 2.0, line_y + 2.0, cell_w - 2.0, 18.0);
                        }
                        let text_size = row.text_size.unwrap_or(14.0);
                        sg::set_viewport(
                            f,
                            Some([cell_x + 3.0, if is_overlay { line_y + 2.0 } else { cell_y }, cell_w - 4.0, 18.0]),
                        );
                        sg::draw_string(
                            f,
                            1.0,
                            9.0 - text_size / 2.0,
                            Some("LEFT"),
                            text_size,
                            "VAR",
                            &format!("^7{}", fmt(format, Some(cell))),
                        );
                        sg::set_viewport(f, None);
                    }
                    line_y += 18.0;
                }
                if rows > 0 {
                    line_y += 2.0;
                }
            }
        }
    }

    /// `self.id`.
    pub fn debug_id(&self) -> &str {
        &self.def.id
    }

    /// `debug`: the state the gate compares.
    fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        let (x, y) = self.get_pos(g);
        let (w, h) = self.get_size(g);
        let subs: Vec<serde_json::Value> = self
            .subs
            .iter()
            .map(|s| {
                let rows: String = s.rows.iter().map(|r| if r.enabled.get() { '1' } else { '0' }).collect();
                serde_json::json!({ "id": s.id, "collapsed": s.collapsed.get(), "rows": rows })
            })
            .collect();
        serde_json::json!({
            "id": self.def.id,
            "enabled": self.enabled.get(),
            "overlay": self.is_overlay.get(),
            "rect": format!("{},{},{},{}", crate::lua::tostring_num(x), crate::lua::tostring_num(y), crate::lua::tostring_num(w), crate::lua::tostring_num(h)),
            "subs": subs,
        })
    }
}

impl Control for CalcSection {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// :187-199
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        sg::set_draw_layer(f, None, -10);
        sg::set_draw_color_str(f, &self.def.colour);
        sg::draw_rect(f, x, y, width, height);
        sg::set_draw_color(f, 0.10, 0.10, 0.10);
        sg::draw_rect(f, x + 2.0, y + 2.0, width - 4.0, height - 4.0);
        self.draw_content(f, x, y, width, view_port, false, no_tooltip);
    }
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over_comp(g).0)
    }
    fn has_on_key_down(&self) -> bool {
        true
    }
    /// :201-221
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let Some(m) = self.host.get_mouse_over_control(g)
            && m.has_on_key_down()
        {
            return m.on_key_down(g, key, false);
        }
        let (m_over, comp) = self.mouse_over_comp(g);
        if key.contains("BUTTON")
            && m_over
            && let Some(k) = comp
        {
            let tab = self.tab();
            tab.set_display_stat(g, k, true);
            return Some(tab.breakdown.clone());
        }
        None
    }
    fn has_on_key_up(&self) -> bool {
        true
    }
    /// :223-232
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let Some(m) = self.host.get_mouse_over_control(g)
            && m.has_on_key_up()
        {
            return m.on_key_up(g, key);
        }
        None
    }
}

/// The `SkillSelect` controls (CalcsTab.lua:46-140).
pub struct SelectControls {
    pub main_socket_group: Rc<DropDownControl>,
    pub main_skill: Rc<DropDownControl>,
    pub main_skill_part: Rc<DropDownControl>,
    pub main_skill_stage_count: Rc<EditControl>,
    pub main_skill_mine_count: Rc<EditControl>,
    pub show_minion: Rc<CheckBoxControl>,
    pub main_skill_minion: Rc<DropDownControl>,
    pub main_skill_minion_library: Rc<ButtonControl>,
    pub main_skill_minion_skill: Rc<DropDownControl>,
    pub mode: Rc<DropDownControl>,
}

pub struct CalcsTab {
    pub host: ControlHost,
    me: Weak<Self>,
    build: Weak<BuildMode>,
    pub undo: UndoHandler<IndexMap<String, InputValue>>,
    /// `self` as an anchor (x, y, width, height of the last `Draw`).
    anchor: Rc<BareControl>,
    pub sections: Vec<Rc<CalcSection>>,
    pub search: Rc<EditControl>,
    pub scroll_bar: Rc<ScrollBarControl>,
    pub breakdown: Rc<CalcBreakdownControl>,
    pub select: SelectControls,
    pub display_data: RefCell<Option<CellKey>>,
    pub display_pinned: Cell<bool>,
}

impl UndoHost for CalcsTab {
    type State = IndexMap<String, InputValue>;
    fn undo_handler(&self) -> &UndoHandler<Self::State> {
        &self.undo
    }
    fn create_undo_state(&self) -> Self::State {
        self.build().engine.borrow().as_ref().expect("engine").calcs_input()
    }
    fn restore_undo_state(&self, state: Self::State) {
        self.build()
            .engine
            .borrow_mut()
            .as_mut()
            .expect("engine")
            .restore_calcs_input(&state);
    }
}

fn buff_item(label: &str, mode: &str) -> DropItem {
    let mut it = DropItem::table(label);
    it.data = Some(Rc::new(mode.to_string()) as Rc<dyn Any>);
    it
}

impl CalcsTab {
    /// :25-161
    pub fn new(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<Self> {
        let tab = Rc::new_cyclic(|me: &Weak<Self>| {
            let anchor_c = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
            let ar: ControlRef = anchor_c.clone();
            let host = ControlHost::new();
            let search = EditControl::new(
                g,
                anchor("TOPLEFT", &ar, "TOPLEFT"),
                rect(4.0, 5.0, 260.0, 20.0),
                EditArgs {
                    init: Some(String::new()),
                    prompt: Some("Search".into()),
                    filter: Some("%c".into()),
                    limit: Some(100),
                    clearable: true,
                    ..Default::default()
                },
            );
            host.add("1", search.clone());
            let b = build.clone();
            let t = me.clone();
            let bf = move || (t.upgrade().expect("calcsTab"), b.upgrade().expect("buildMode"));
            let empty = || rect(0.0, 0.0, 0.0, 16.0);
            let with_w = |w: f64| rect(0.0, 0.0, w, 16.0);
            let _ = empty;
            let bf1 = bf.clone();
            let main_socket_group = DropDownControl::new(
                g,
                None,
                with_w(300.0),
                Vec::new(),
                Some(Rc::new(move |_g: &mut Gfx, index: usize, _v: &DropItem| {
                    let (t, b) = bf1();
                    b.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_calcs_input("skill_number", Some(InputValue::Num(index as f64)));
                    t.add_undo_state(false);
                    b.build_flag.set(true);
                })),
                None,
                false,
            );
            let edit = |f: fn(&mut pob_engine::engine::Engine, usize, &DropItem) -> Result<(), String>, undo: bool| {
                let bf = bf.clone();
                Some(Rc::new(move |_g: &mut Gfx, index: usize, v: &DropItem| {
                    let (t, b) = bf();
                    f(b.engine.borrow_mut().as_mut().expect("engine"), index, v).unwrap_or_else(|e| panic!("{e}"));
                    if undo {
                        t.add_undo_state(false);
                    }
                    b.build_flag.set(true);
                }) as Rc<dyn Fn(&mut Gfx, usize, &DropItem)>)
            };
            let main_skill = DropDownControl::new(
                g,
                None,
                with_w(300.0),
                Vec::new(),
                edit(|e, i, _| e.set_main_active_skill_calcs(i as f64), false),
                None,
                false,
            );
            let main_skill_part = DropDownControl::new(
                g,
                None,
                with_w(250.0),
                Vec::new(),
                edit(|e, i, _| e.set_skill_part_calcs(i as f64), true),
                None,
                false,
            );
            let num_edit = |f: fn(&mut pob_engine::engine::Engine, Option<f64>) -> Result<(), String>| {
                let bf = bf.clone();
                EditArgs {
                    filter: Some("%D".into()),
                    change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                        let (t, b) = bf();
                        f(b.engine.borrow_mut().as_mut().expect("engine"), buf.parse::<f64>().ok()).unwrap_or_else(|e| panic!("{e}"));
                        t.add_undo_state(false);
                        b.build_flag.set(true);
                    })),
                    ..Default::default()
                }
            };
            let main_skill_stage_count = EditControl::new(
                g,
                None,
                rect(0.0, 0.0, 52.0, 16.0),
                num_edit(|e, c| e.set_skill_stage_count_calcs(c)),
            );
            let main_skill_mine_count = EditControl::new(
                g,
                None,
                rect(0.0, 0.0, 52.0, 16.0),
                num_edit(|e, c| e.set_skill_mine_count_calcs(c)),
            );
            let bf2 = bf.clone();
            let show_minion = CheckBoxControl::new(
                g,
                None,
                [0.0, 0.0, 18.0],
                None,
                Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                    let (t, b) = bf2();
                    b.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_calcs_input("showMinion", Some(InputValue::Bool(state)));
                    t.add_undo_state(false);
                })),
                Some("Show stats for the minion instead of the player."),
                false,
            );
            let main_skill_minion = DropDownControl::new(
                g,
                None,
                with_w(160.0),
                Vec::new(),
                edit(
                    |e, _, v| {
                        let entry = v
                            .data
                            .as_ref()
                            .and_then(|d| d.downcast_ref::<MinionEntry>())
                            .cloned()
                            .unwrap_or_else(|| MinionEntry::Text(v.label.clone().unwrap_or_default()));
                        e.set_skill_minion_calcs(&entry)
                    },
                    true,
                ),
                None,
                false,
            );
            let main_skill_minion_library = ButtonControl::new(None, with_w(100.0), "Manage Spectres...".to_string(), |g| {
                not_ported(g, "Modules/Build.lua:1522-1551 OpenSpectreLibrary (MinionListControl): U5");
            });
            let main_skill_minion_skill = DropDownControl::new(
                g,
                None,
                with_w(200.0),
                Vec::new(),
                edit(|e, i, _| e.set_skill_minion_skill_calcs(i as f64), true),
                None,
                false,
            );
            let bf3 = bf.clone();
            let mode = DropDownControl::new(
                g,
                None,
                with_w(100.0),
                BUFF_MODES.iter().map(|(l, m)| buff_item(l, m)).collect(),
                Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
                    let (t, b) = bf3();
                    let m = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned().expect("buffMode");
                    b.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_calcs_input("misc_buffMode", Some(InputValue::Str(m)));
                    t.add_undo_state(false);
                    b.build_flag.set(true);
                })),
                Some(MODE_TOOLTIP),
                false,
            );
            let mut named: IndexMap<String, ControlRef> = IndexMap::new();
            named.insert("mainSocketGroup".into(), main_socket_group.clone());
            named.insert("mainSkill".into(), main_skill.clone());
            named.insert("mainSkillPart".into(), main_skill_part.clone());
            named.insert("mainSkillStageCount".into(), main_skill_stage_count.clone());
            named.insert("mainSkillMineCount".into(), main_skill_mine_count.clone());
            named.insert("showMinion".into(), show_minion.clone());
            named.insert("mainSkillMinion".into(), main_skill_minion.clone());
            named.insert("mainSkillMinionLibrary".into(), main_skill_minion_library.clone());
            named.insert("mainSkillMinionSkill".into(), main_skill_minion_skill.clone());
            named.insert("mode".into(), mode.clone());
            let bu = build.upgrade().expect("buildMode");
            let eng = bu.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let collapsed = |s: &str, id: &str| e.calcs_collapsed(s, id);
            let mut list = Vec::new();
            for (i, def) in sections().iter().enumerate() {
                let s = CalcSection::new(me.clone(), i, def, &named, &collapsed);
                host.add(&(i + 2).to_string(), s.clone());
                list.push(s);
            }
            drop(eng);
            host.add("search", search.clone());
            let breakdown = CalcBreakdownControl::new(me.clone(), BdEnv::Calcs);
            host.add("breakdown", breakdown.clone());
            let scroll_bar = ScrollBarControl::new(
                anchor("TOPRIGHT", &ar, "TOPRIGHT"),
                rect(0.0, 0.0, 18.0, 0.0),
                Some(50.0),
                Some("VERTICAL"),
                true,
            );
            host.add("scrollBar", scroll_bar.clone());
            Self {
                host,
                me: me.clone(),
                build: build.clone(),
                undo: UndoHandler::default(),
                anchor: anchor_c,
                sections: list,
                search,
                scroll_bar,
                breakdown,
                select: SelectControls {
                    main_socket_group,
                    main_skill,
                    main_skill_part,
                    main_skill_stage_count,
                    main_skill_mine_count,
                    show_minion,
                    main_skill_minion,
                    main_skill_minion_library,
                    main_skill_minion_skill,
                    mode,
                },
                display_data: RefCell::new(None),
                display_pinned: Cell::new(false),
            }
        });
        tab.reset_undo();
        tab
    }

    pub fn build(&self) -> Rc<BuildMode> {
        self.build.upgrade().expect("buildMode")
    }

    /// `actor = input.showMinion and calcsEnv.minion or calcsEnv.player` with `CheckFlag`'s player.
    pub fn with_actor<R>(&self, f: impl FnOnce(&CheckCtx<'_, '_>) -> R) -> R {
        let b = self.build();
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let run = e.calcs_run();
        let player = player_actor(&run.env, &run.state.player).unwrap_or_else(|err| panic!("{err}"));
        if e.calcs_show_minion()
            && let Some(ms) = &run.state.minion
        {
            let ms = ms.borrow();
            if let Some(minion) = minion_actor(&run.env, &ms).unwrap_or_else(|err| panic!("{err}")) {
                return f(&CheckCtx {
                    actor: &minion,
                    player_flags: player.skill_flags,
                });
            }
        }
        f(&CheckCtx {
            actor: &player,
            player_flags: player.skill_flags,
        })
    }

    pub fn cell_def(&self, k: CellKey) -> &'static pob_engine::calcs::Cell {
        &sections()[k.0].sub_sections[k.1].rows[k.2].cells[k.3]
    }

    /// `colData.x`, `.y`, `.width`.
    pub fn cell_rect(&self, k: CellKey) -> (f64, f64, f64) {
        let c = &self.sections[k.0].subs[k.1].rows[k.2].cells[k.3];
        (c.x.get(), c.y.get(), c.width.get())
    }

    /// The `updateFunc` of `SkillSelect` (:144-148).
    fn update_skill_select(&self, g: &mut Gfx) {
        let s = &self.select;
        let (sel, show, mode) = {
            let b = self.build();
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let sel = e.skill_select_calcs().unwrap_or_else(|err| panic!("{err}"));
            let input = e.calcs_input();
            let mode = match input.get("misc_buffMode") {
                Some(InputValue::Str(m)) => Some(m.clone()),
                _ => None,
            };
            (sel, e.calcs_show_minion(), mode)
        };
        apply_skill_select(
            g,
            &SkillControls {
                main_socket_group: &s.main_socket_group,
                main_skill: &s.main_skill,
                main_skill_part: &s.main_skill_part,
                main_skill_stage_count: &s.main_skill_stage_count,
                main_skill_mine_count: &s.main_skill_mine_count,
                main_skill_minion: &s.main_skill_minion,
                main_skill_minion_library: &s.main_skill_minion_library,
                main_skill_minion_skill: &s.main_skill_minion_skill,
            },
            &sel,
        );
        s.show_minion.state.set(show);
        if let Some(m) = mode {
            sel_by_value(&s.mode, &m);
        }
    }

    /// :435-438
    fn search_match(&self, txt: &str) -> bool {
        let search = self.search.buf.borrow().to_ascii_lowercase();
        !search.is_empty() && crate::lua::is_match(&txt.to_ascii_lowercase(), &search)
    }

    /// :377-381
    pub fn clear_display_stat(&self, g: &mut Gfx) {
        *self.display_data.borrow_mut() = None;
        self.display_pinned.set(false);
        self.breakdown.set_breakdown_data(g, None, false, None);
    }

    /// :383-394 `SetDisplayStat(displayData)` of a hover.
    pub fn set_display_hover(&self, g: &mut Gfx, k: CellKey) {
        if self.display_pinned.get() {
            return;
        }
        *self.display_data.borrow_mut() = Some(k);
        self.display_pinned.set(false);
        self.breakdown.set_breakdown_data(g, Some(Source::Cell(k)), false, None);
    }

    /// :383-394 `SetDisplayStat(displayData, true)`.
    pub fn set_display_stat(&self, g: &mut Gfx, k: CellKey, pin: bool) {
        if !pin && self.display_pinned.get() {
            return;
        }
        if pin && self.display_pinned.get() && *self.display_data.borrow() == Some(k) {
            self.clear_display_stat(g);
            return;
        }
        *self.display_data.borrow_mut() = Some(k);
        self.display_pinned.set(pin);
        self.breakdown.set_breakdown_data(g, Some(Source::Cell(k)), pin, None);
    }

    /// :464-467: what `BuildOutput` does for the shown popup.
    pub fn after_build_output(&self, g: &mut Gfx) {
        let d = *self.display_data.borrow();
        if let Some(k) = d {
            self.breakdown.set_breakdown_data(g, None, false, None);
            self.breakdown
                .set_breakdown_data(g, Some(Source::Cell(k)), self.display_pinned.get(), None);
        }
    }

    /// :226-368 `Draw`.
    #[allow(clippy::needless_range_loop)]
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let a = &self.anchor.base;
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        let base_x = view_port.x + 4.0;
        let base_y = view_port.y + 30.0;
        let col_width = 230.0;
        let portrait = main().portrait_mode.get();
        let mut max_col = (view_port.width / (col_width + 8.0)).floor();
        if portrait {
            max_col = 3.0;
        }
        let mut col_y: [Option<f64>; 8] = [None; 8];
        let mut max_y: f64 = 0.0;
        let group = |s: &CalcSection| s.def.group as i64;
        let wc = |s: &CalcSection| s.def.width_cols as usize;
        for section in &self.sections {
            section.update_size(f);
            if section.enabled.get() && !section.is_overlay.get() {
                let mut col: Option<usize> = None;
                match group(section) {
                    1 => {
                        let mut c0 = 1;
                        if section.def.width() == col_width {
                            let mut min_y = col_y[1].unwrap_or(base_y);
                            for c in 2..=3 {
                                if col_y[c].unwrap_or(base_y) < min_y {
                                    c0 = c;
                                    min_y = col_y[c].unwrap_or(base_y);
                                }
                            }
                        } else {
                            for c in 2..=3 {
                                col_y[c0] = Some(col_y[c0].unwrap_or(base_y).max(col_y[c].unwrap_or(base_y)));
                            }
                        }
                        col = Some(c0);
                    }
                    2 if max_col >= 4.0 => col = Some(4),
                    3 if max_col >= 5.0 => col = Some(5),
                    _ => {}
                }
                if let Some(col) = col {
                    let y = col_y[col].unwrap_or(base_y);
                    section.base.x.set(base_x + (col_width + 8.0) * (col as f64 - 1.0));
                    section.base.y.set(y);
                    let h = section.base.height.get(f);
                    for c in col..col + wc(section) {
                        col_y[c] = Some(y + h + 8.0);
                    }
                    max_y = max_y.max(col_y[col].expect("just set"));
                }
            }
        }
        if max_col < 5.0 {
            let m = col_y[1]
                .expect("colY[1]")
                .max(col_y[2].expect("colY[2]"))
                .max(col_y[3].expect("colY[3]"));
            for c in col_y.iter_mut().take(4).skip(1) {
                *c = Some(m);
            }
            for section in &self.sections {
                if section.enabled.get() && !section.is_overlay.get() && (portrait && group(section) == 2 || group(section) == 3) {
                    let mut col = 3;
                    let h = section.base.height.get(f);
                    if col_y[col].expect("colY") + h + 4.0 >= (view_port.y + view_port.height).max(max_y) {
                        let mut min_y = col_y[col].expect("colY");
                        for c in (1..=3).rev() {
                            if col_y[c].expect("colY") < min_y {
                                col = c;
                                min_y = col_y[c].expect("colY");
                            }
                        }
                    }
                    let y = col_y[col].expect("colY");
                    section.base.x.set(base_x + (col_width + 8.0) * (col as f64 - 1.0));
                    section.base.y.set(y);
                    col_y[col] = Some(y + h + 8.0);
                    max_y = max_y.max(y + h + 8.0);
                }
            }
        }
        self.scroll_bar.base.height.set(view_port.height);
        self.scroll_bar.set_content_dimension(f, max_y - (base_y - 26.0), view_port.height);
        let offset = self.scroll_bar.offset.get();
        for section in &self.sections {
            if !section.is_overlay.get() {
                section.base.y.set(section.base.y.get(f) - offset);
                section.update_pos(f);
            }
        }
        self.search.base.y.set(4.0 - offset);
        let keys: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in keys {
            if sg::is_key_down(f, "CTRL") {
                match key.as_str() {
                    "z" => {
                        self.undo();
                        self.build().build_flag.set(true);
                    }
                    "y" => {
                        self.redo();
                        self.build().build_flag.set(true);
                    }
                    "f" => self.host.select_control(f, Some(self.search.clone())),
                    _ => {}
                }
            }
        }
        self.host.process_controls_input(f, input_events, view_port);
        let ups: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyUp { key } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in ups {
            if ScrollBarControl::is_scroll_down_key(&key) {
                self.scroll_bar.scroll(1.0);
            } else if ScrollBarControl::is_scroll_up_key(&key) {
                self.scroll_bar.scroll(-1.0);
            }
        }
        main().draw_background(f, view_port);
        if !self.display_pinned.get() {
            *self.display_data.borrow_mut() = None;
        }
        let breakdown = &self.breakdown;
        let overlay_breakdown = matches!(&*breakdown.source.borrow(), Some(Source::Cell(k)) if self.sections[k.0].is_overlay.get());
        if overlay_breakdown {
            breakdown.base.shown.set(false);
        }
        let sel = self.host.sel();
        self.host.draw_controls(f, view_port, sel.as_ref());
        if overlay_breakdown {
            breakdown.base.shown.set(true);
        }
        if self.display_data.borrow().is_some() {
            if self.display_pinned.get() && self.host.sel().is_none() {
                self.host.select_control(f, Some(self.breakdown.clone()));
            }
        } else {
            self.breakdown.set_breakdown_data(f, None, false, None);
        }
        let _ = &self.me;
    }

    /// `cursorCell` of the gate: inside the first enabled row of that label.
    pub fn debug_cell_pos(&self, id: &str, label: &str, col: usize) -> (f64, f64) {
        let s = self
            .sections
            .iter()
            .find(|s| s.def.id == id)
            .unwrap_or_else(|| panic!("section {id}"));
        for (i, sub) in s.def.sub_sections.iter().enumerate() {
            for (j, row) in sub.rows.iter().enumerate() {
                if s.subs[i].rows[j].enabled.get() && row.label.as_deref() == Some(label) {
                    let c = &s.subs[i].rows[j].cells[col - 1];
                    return (c.x.get() + 5.0, c.y.get() + 5.0);
                }
            }
        }
        panic!("cursorCell: no row {label}");
    }

    /// `cursorControl` of the gate.
    pub fn debug_control_pos(&self, g: &Gfx, id: &str, name: &str) -> (f64, f64) {
        let s = self
            .sections
            .iter()
            .find(|s| s.def.id == id)
            .unwrap_or_else(|| panic!("section {id}"));
        let c = s.host.get(name).unwrap_or_else(|| panic!("control {name}"));
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        ((x + w / 2.0).floor(), (y + h / 2.0).floor())
    }

    /// The state the gate compares (the same fields as `calcs-runner.lua`).
    pub fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        let drop = |c: &DropDownControl| {
            let list: Vec<String> = c.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect();
            serde_json::json!({ "shown": c.is_shown(g), "enabled": c.is_enabled(g), "sel": c.sel_index.get(), "list": list })
        };
        let s = &self.select;
        let input: serde_json::Map<String, serde_json::Value> = {
            let b = self.build();
            let eng = b.engine.borrow();
            let i = eng.as_ref().expect("engine").calcs_input();
            i.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            InputValue::Num(n) => serde_json::json!(n),
                            InputValue::Bool(b) => serde_json::json!(b),
                            InputValue::Str(s) => serde_json::json!(s),
                        },
                    )
                })
                .collect()
        };
        let sel = self.host.sel().map_or("nil".to_string(), |c| {
            let is = |o: &dyn Control| crate::control::same(o, &*c);
            if is(&*self.search) {
                "search".to_string()
            } else if is(&*self.breakdown) {
                "breakdown".to_string()
            } else if is(&*self.scroll_bar) {
                "scrollBar".to_string()
            } else if let Some(s) = self.sections.iter().find(|s| is(&***s)) {
                s.def.id.clone()
            } else {
                "other".to_string()
            }
        });
        let display = self
            .display_data
            .borrow()
            .map(|k| format!("{}/{}/{}/{}", self.sections[k.0].def.id, k.1 + 1, k.2 + 1, k.3 + 1));
        serde_json::json!({
            "input": input,
            "sel": sel,
            "scroll": self.scroll_bar.offset.get(),
            "search": &*self.search.buf.borrow(),
            "pinned": self.display_pinned.get(),
            "display": display,
            "skill": {
                "mainSocketGroup": drop(&s.main_socket_group),
                "mainSkill": drop(&s.main_skill),
                "mainSkillPart": drop(&s.main_skill_part),
                "mainSkillMinion": drop(&s.main_skill_minion),
                "mainSkillMinionSkill": drop(&s.main_skill_minion_skill),
                "mode": s.mode.sel_index.get(),
                "showMinion": s.show_minion.state.get(),
                "stage": if s.main_skill_stage_count.is_shown(g) { s.main_skill_stage_count.buf.borrow().clone() } else { "hidden".to_string() },
                "mine": if s.main_skill_mine_count.is_shown(g) { s.main_skill_mine_count.buf.borrow().clone() } else { "hidden".to_string() },
            },
            "sections": self.sections.iter().map(|x| x.debug_dump(g)).collect::<Vec<_>>(),
            "breakdown": self.breakdown.debug_dump(g),
        })
    }
}
