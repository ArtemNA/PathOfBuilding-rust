//! The Config view of `Classes/CompareTab.lua`: `makeConfigControl`, `RebuildConfigControls`, `CopyCompareConfig`, `LayoutConfigView`, `DrawConfig` (:1133-1242, :2031-2225, :4936-5032).

use std::rc::Rc;

use pob_calc::pob_build::InputValue;
use pob_data::config_options::{ConfigVar, config_options};
use pob_gfx::{Frame, Gfx};

use super::power::scalar_string;
use super::{CompareEntry, CompareTab};
use crate::colors::color_code;
use crate::control::{ControlRef, Rect, rect};
use crate::controls::basic::CheckBoxControl;
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::modules::build::BuildMode;
use crate::sg;

const ROW_HEIGHT: f64 = 22.0;
const COLUMN_HEADER_HEIGHT: f64 = 20.0;
const FIXED_HEADER_HEIGHT: f64 = 82.0;
const SECTION_WIDTH: f64 = 560.0;
const SECTION_GAP: f64 = 24.0;
const SECTION_COLUMN_GAP: f64 = 10.0;
const SECTION_INNER_PAD: f64 = 20.0;
const LABEL_OFFSET: f64 = 10.0;
const COL2: f64 = 234.0;
const COL3: f64 = 400.0;

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

/// One side's control of an option.
#[derive(Clone)]
pub enum VarControl {
    Check(Rc<CheckBoxControl>),
    Edit(Rc<EditControl>),
    Drop(Rc<DropDownControl>),
}

impl VarControl {
    fn control(&self) -> ControlRef {
        match self {
            VarControl::Check(c) => c.clone(),
            VarControl::Edit(c) => c.clone(),
            VarControl::Drop(c) => c.clone(),
        }
    }

    /// `syncControlValue` (:2031-2044).
    fn sync(&self, g: &mut Gfx, var: &ConfigVar, val: Option<&InputValue>) {
        match self {
            VarControl::Check(c) => c.state.set(matches!(val, Some(v) if !matches!(v, InputValue::Bool(false)))),
            VarControl::Edit(c) => {
                let text = val.map_or_else(String::new, input_str);
                if !c.base.has_focus.get() && text != *c.buf.borrow() {
                    c.set_text(g, &text, false);
                }
            }
            VarControl::Drop(c) => sel_by_val(c, var, val),
        }
    }
}

fn input_str(v: &InputValue) -> String {
    match v {
        InputValue::Num(n) => pob_lua::tostring_num(*n),
        InputValue::Bool(b) => b.to_string(),
        InputValue::Str(s) => s.to_string(),
    }
}

/// `control:SelByValue(val or list[1].val, "val")`.
fn sel_by_val(c: &DropDownControl, var: &ConfigVar, val: Option<&InputValue>) {
    let Some(list) = &var.list else { return };
    let v = val.map(input_str).or_else(|| list.first().map(|i| scalar_string(&i.val)));
    if let Some(i) = list.iter().position(|e| Some(scalar_string(&e.val)) == v) {
        c.sel_index.set(i + 1);
    }
}

/// `value.val` as an input.
fn item_val(e: &pob_data::config_options::ConfigListEntry) -> Option<InputValue> {
    match &e.val {
        pob_data::wire::Scalar::Num(n) => Some(InputValue::Num(*n)),
        pob_data::wire::Scalar::Bool(b) => Some(InputValue::Bool(*b)),
        pob_data::wire::Scalar::Str(s) => Some(InputValue::Str(s.as_str().into())),
    }
}

/// What the callbacks of `makeConfigControl` do (:1137-1163): `inputTable[var] = value`, `configTab:UpdateControls()`, `BuildModList()`, `buildObj.buildFlag = true`.
fn write_input(b: &BuildMode, g: &mut Gfx, var: &str, v: Option<InputValue>) {
    b.engine.borrow_mut().as_mut().expect("engine").config_write_input(var, v);
    let tab = b.config_tab.borrow().clone().expect("configTab");
    tab.update_controls(g);
    tab.build_mod_list(g);
    b.build_flag.set(true);
}

/// `makeConfigControl` (:1133-1176).
fn make_config_control(g: &mut Gfx, var: &'static ConfigVar, b: &Rc<BuildMode>) -> Option<VarControl> {
    let name: &'static str = var.var.as_deref()?;
    let raw = b.engine.borrow().as_ref().expect("engine").config_state().input.get(name).cloned();
    let w = Rc::downgrade(b);
    let control = match var.ty.as_deref() {
        Some("check") => {
            let c = CheckBoxControl::new(
                g,
                None,
                [0.0, 0.0, 18.0],
                None,
                Some(Rc::new(move |g: &mut Gfx, state: bool| {
                    write_input(&w.upgrade().expect("build"), g, name, Some(InputValue::Bool(state)));
                })),
                None,
                matches!(raw, Some(ref v) if !matches!(v, InputValue::Bool(false))),
            );
            VarControl::Check(c)
        }
        Some(t @ ("count" | "integer" | "countAllowZero" | "float")) => {
            let filter = match t {
                "integer" => "^%-%d",
                "float" => "^%d.",
                _ => "%D",
            };
            VarControl::Edit(EditControl::new(
                g,
                None,
                rect(0.0, 0.0, 90.0, 18.0),
                EditArgs {
                    init: Some(raw.as_ref().map_or_else(String::new, input_str)),
                    filter: Some(filter.into()),
                    limit: Some(7),
                    change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _| {
                        write_input(&w.upgrade().expect("build"), g, name, pob_lua::tonumber(buf).map(InputValue::Num));
                    })),
                    ..EditArgs::default()
                },
            ))
        }
        Some("list") if var.list.is_some() => {
            let list: Vec<DropItem> = var.list.as_ref().expect("list").iter().map(|e| DropItem::table(&e.label)).collect();
            let d = DropDownControl::new(
                g,
                None,
                rect(0.0, 0.0, 150.0, 18.0),
                list,
                Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                    let e = &var.list.as_ref().expect("list")[index - 1];
                    write_input(&w.upgrade().expect("build"), g, name, item_val(e));
                })),
                None,
                false,
            );
            sel_by_val(&d, var, raw.as_ref());
            VarControl::Drop(d)
        }
        _ => return None,
    };
    // :1170-1173 the tooltip of the source control of ConfigTab
    let tab = b.config_tab.borrow().clone().expect("configTab");
    match &control {
        VarControl::Check(c) => {
            let t = Rc::downgrade(&tab);
            c.tip
                .set_tooltip_func(move |tt, g, _s| t.upgrade().expect("configTab").tooltip_of_var(tt, g, name, "", None));
            c.base.shown.set(false);
        }
        VarControl::Edit(c) => {
            let t = Rc::downgrade(&tab);
            c.tip
                .set_tooltip_func(move |tt, g, ()| t.upgrade().expect("configTab").tooltip_of_var(tt, g, name, "", None));
            c.base.shown.set(false);
        }
        VarControl::Drop(c) => {
            let t = Rc::downgrade(&tab);
            c.tip.set_tooltip_func(move |tt, g, (mode, _i, item)| {
                t.upgrade().expect("configTab").tooltip_of_var(tt, g, name, mode, item.as_ref())
            });
            c.base.shown.set(false);
        }
    }
    Some(control)
}

pub struct ConfigInfo {
    pub var: &'static ConfigVar,
    pub primary: VarControl,
    pub compare: VarControl,
}

pub struct ConfigSectionInfo {
    pub name: String,
    pub col: f64,
    pub items: Vec<Rc<ConfigInfo>>,
}

struct LaidRow {
    info: Rc<ConfigInfo>,
    is_diff: bool,
}

pub struct LaidSection {
    name: String,
    col: f64,
    rows: Vec<LaidRow>,
    height: f64,
    diff_count: usize,
    x: f64,
    y: f64,
}

/// `NormalizeConfigVals` (:1123-1130) through `tostring`.
fn normalized_str(var: &ConfigVar, v: Option<&InputValue>) -> String {
    match var.ty.as_deref() {
        Some("check") => v.map_or_else(|| "false".to_string(), input_str),
        Some("count" | "integer" | "float") => v.map_or_else(|| "0".to_string(), input_str),
        _ => v.map_or_else(|| "nil".to_string(), input_str),
    }
}

impl CompareTab {
    /// `RebuildConfigControls` (:1179-1228).
    fn rebuild_config_controls(&self, g: &mut Gfx, entry: &Rc<CompareEntry>) {
        for (var, _) in self.config_controls.borrow().iter() {
            self.host.remove(&format!("cfg_p_{var}"));
            self.host.remove(&format!("cfg_c_{var}"));
        }
        self.config_controls.borrow_mut().clear();
        self.config_sections.borrow_mut().clear();
        let primary = self.primary();
        let opts = config_options();
        let mut current: Option<ConfigSectionInfo> = None;
        let mut last_section = String::new();
        for var in &opts.vars {
            if var.section != last_section {
                last_section.clone_from(&var.section);
                if let Some(s) = current.take() {
                    self.config_sections.borrow_mut().push(s);
                }
                if var.section != "Custom Modifiers" && var.section != "Map Modifiers and Player Debuffs" {
                    let col = opts.sections.iter().find(|s| s.section == var.section).map_or(0.0, |s| s.col);
                    current = Some(ConfigSectionInfo {
                        name: var.section.clone(),
                        col,
                        items: Vec::new(),
                    });
                }
            }
            let Some(sec) = current.as_mut() else { continue };
            let Some(name) = var.var.as_deref() else { continue };
            if var.ty.as_deref() == Some("text") {
                continue;
            }
            let (Some(p), Some(c)) = (make_config_control(g, var, &primary), make_config_control(g, var, &entry.b)) else {
                continue;
            };
            self.host.add(&format!("cfg_p_{name}"), p.control());
            self.host.add(&format!("cfg_c_{name}"), c.control());
            let info = Rc::new(ConfigInfo {
                var,
                primary: p,
                compare: c,
            });
            self.config_controls.borrow_mut().push((name.to_string(), info.clone()));
            sec.items.push(info);
        }
        if let Some(s) = current.take() {
            self.config_sections.borrow_mut().push(s);
        }
    }

    /// `CopyCompareConfig` (:1231-1242).
    pub(super) fn copy_compare_config(&self, g: &mut Gfx) {
        let Some(entry) = self.active_compare() else { return };
        let input: Vec<(String, InputValue)> = {
            let eng = entry.b.engine.borrow();
            eng.as_ref()
                .expect("engine")
                .config_state()
                .input
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        let p = self.primary();
        {
            let mut eng = p.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            for (k, v) in input {
                e.config_write_input(&k, Some(v));
            }
        }
        let tab = p.config_tab.borrow().clone().expect("configTab");
        tab.update_controls(g);
        tab.build_mod_list(g);
        p.build_flag.set(true);
        self.config_needs_rebuild.set(true);
    }

    /// `LayoutConfigView` (:2047-2225).
    #[allow(clippy::too_many_lines)]
    pub(super) fn layout_config_view(&self, g: &mut Gfx, vp: &Rect, entry: &Rc<CompareEntry>) {
        let id = self.active.get();
        if self.config_compare_id.get() != Some(id) || self.config_needs_rebuild.get() {
            self.rebuild_config_controls(g, entry);
            self.config_compare_id.set(Some(id));
            self.config_needs_rebuild.set(false);
        }
        let primary = self.primary();
        let input_of = |b: &BuildMode, var: &str| b.engine.borrow().as_ref().expect("engine").config_state().input.get(var).cloned();
        let controls: Vec<(String, Rc<ConfigInfo>)> = self.config_controls.borrow().clone();
        for (var, info) in &controls {
            info.primary.sync(g, info.var, input_of(&primary, var).as_ref());
            info.compare.sync(g, info.var, input_of(&entry.b, var).as_ref());
        }
        let c = self.c();
        let (row1, row2) = (vp.y + 4.0, vp.y + 28.0);
        let left = vp.x + 10.0;
        let set_x = vp.x + 80.0;
        let set_w = 225.0;
        let input_end = set_x + set_w;
        let action = input_end + 10.0;
        c.copy_config_btn.base.x.set(action);
        c.copy_config_btn.base.y.set(row1);
        c.config_toggle_btn.base.x.set(action);
        c.config_toggle_btn.base.y.set(row2);
        c.config_search_edit.base.x.set(left);
        c.config_search_edit.base.width.set(input_end - left);
        c.config_search_edit.base.y.set(row2);
        {
            let eng = primary.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let ids = e
                .config_set_order()
                .into_iter()
                .map(|i| (i, e.config_set_title(i).unwrap_or_else(|| "Default".into())))
                .collect();
            super::populate(ids, e.config_active_set(), &c.config_primary_set_select, g);
        }
        c.config_primary_set_label.base.x.set(left);
        c.config_primary_set_label.base.y.set(row1 + 2.0);
        c.config_primary_set_select.base.x.set(set_x);
        c.config_primary_set_select.base.width.set(set_w);
        c.config_primary_set_select.base.y.set(row1);
        let column_stride = SECTION_WIDTH + SECTION_COLUMN_GAP;
        let scrollable_h = vp.height - FIXED_HEADER_HEIGHT;
        for (_, info) in &controls {
            info.primary.control().base().shown.set(false);
            info.compare.control().base().shown.set(false);
        }
        // the search: `searchMatch` over `label:lower()`
        let search = {
            let lower = c.config_search_edit.buf.borrow().to_ascii_lowercase();
            let esc = pob_lua::gsub(lower.as_bytes(), b"[%-%.%+%[%]%$%^%%%?%*]", pob_lua::Repl::Str(b"%%%0"), None)
                .expect("pattern")
                .0;
            let s = String::from_utf8_lossy(&esc).into_owned();
            s.bytes()
                .any(|b| !b.is_ascii_whitespace())
                .then(|| crate::tabs::config::generate_or_patterns(&s))
        };
        let search_match = |var: &ConfigVar| {
            let Some(p) = &search else { return true };
            let label = var.label.as_deref().unwrap_or("").to_ascii_lowercase();
            p.iter()
                .any(|x| matches!(pob_lua::match_(label.as_bytes(), x.as_bytes(), None), Ok(Some(_))))
        };
        let ptab = primary.config_tab.borrow().clone().expect("configTab");
        let ctab = entry.b.config_tab.borrow().clone().expect("configTab");
        let toggle = self.config_toggle.get();
        let mut visible: Vec<LaidSection> = Vec::new();
        for section in self.config_sections.borrow().iter() {
            let (mut diffs, mut commons) = (Vec::new(), Vec::new());
            for info in &section.items {
                if !search_match(info.var) {
                    continue;
                }
                let name = info.var.var.as_deref().expect("var");
                let (pv, cv) = (input_of(&primary, name), input_of(&entry.b, name));
                if normalized_str(info.var, pv.as_ref()) != normalized_str(info.var, cv.as_ref()) {
                    diffs.push(LaidRow {
                        info: info.clone(),
                        is_diff: true,
                    });
                } else {
                    let relevant = ptab.is_relevant_var(name) || ctab.is_relevant_var(name);
                    if relevant || (toggle && !ptab.is_show_all_excluded_var(name)) {
                        commons.push(LaidRow {
                            info: info.clone(),
                            is_diff: false,
                        });
                    }
                }
            }
            let diff_count = diffs.len();
            diffs.extend(commons);
            if !diffs.is_empty() {
                let height = SECTION_INNER_PAD + diffs.len() as f64 * ROW_HEIGHT + 8.0;
                visible.push(LaidSection {
                    name: section.name.clone(),
                    col: section.col,
                    rows: diffs,
                    height,
                    diff_count,
                    x: 0.0,
                    y: 0.0,
                });
            }
        }
        // :2165-2194
        let max_col = (((vp.width - 10.0) / column_stride).floor() as usize).max(1);
        let mut col_y: Vec<f64> = vec![0.0; max_col.max(8) + 1];
        let mut max_y: f64 = 0.0;
        for sec in &mut visible {
            let h = sec.height;
            let pref = sec.col as usize;
            let col = if pref > 0
                && col_y.get(pref).copied().unwrap_or(0.0) + h + 28.0 <= scrollable_h
                && 10.0 + sec.col * column_stride <= vp.width
            {
                pref
            } else {
                let mut col = 1;
                for k in 2..=max_col {
                    if col_y[k] < col_y[col] {
                        col = k;
                    }
                }
                col
            };
            sec.x = 10.0 + (col as f64 - 1.0) * column_stride;
            sec.y = col_y[col] + SECTION_GAP;
            col_y[col] += h + SECTION_GAP;
            max_y = max_y.max(col_y[col]);
        }
        // :2197-2221
        let top = vp.y + FIXED_HEADER_HEIGHT;
        let bottom = vp.y + vp.height;
        let clip = [vp.x, top, vp.width, bottom - top];
        let w = self.me.clone();
        for sec in &visible {
            let ax = vp.x + sec.x;
            let mut row_y = sec.y + SECTION_INNER_PAD;
            for row in &sec.rows {
                let y = vp.y + FIXED_HEADER_HEIGHT + row_y - self.scroll_y.get();
                for (ctl, x) in [(row.info.primary.control(), ax + COL2), (row.info.compare.control(), ax + COL3)] {
                    let b = ctl.base();
                    b.x.set(x);
                    b.y.set(y);
                    let w = w.clone();
                    b.shown.set_fn(move |_| {
                        let t = w.upgrade().expect("compareTab");
                        y + ROW_HEIGHT > top && y < bottom && *t.view_mode.borrow() == "CONFIG" && t.active_compare().is_some()
                    });
                    b.mouse_clip.set(Some(clip));
                }
                row_y += ROW_HEIGHT;
            }
        }
        *self.config_section_layout.borrow_mut() = visible;
        self.config_total_content_height.set(max_y + SECTION_GAP);
    }

    /// `DrawConfig` (:4936-5032).
    pub(super) fn draw_config(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>, header_only: bool) {
        sg::set_viewport(f, Some([vp.x, vp.y, vp.width, FIXED_HEADER_HEIGHT]));
        sg::set_draw_color(f, 0.05, 0.05, 0.05);
        sg::draw_rect(f, 0.0, 0.0, vp.width, FIXED_HEADER_HEIGHT);
        let col_y = 54.0;
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        let primary = self.primary();
        let name = Self::short_build_name(primary.build_name.borrow().as_deref());
        sg::draw_string(
            f,
            10.0 + COL3 - 8.0,
            col_y,
            Some("RIGHT_X"),
            COLUMN_HEADER_HEIGHT,
            "VAR",
            &format!("{}{name}", code("POSITIVE")),
        );
        sg::draw_string(
            f,
            10.0 + COL3,
            col_y,
            Some("LEFT"),
            COLUMN_HEADER_HEIGHT,
            "VAR",
            &format!("{}{}", code("WARNING"), entry.label),
        );
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, 4.0, col_y + COLUMN_HEADER_HEIGHT + 4.0, vp.width - 8.0, 2.0);
        let scroll_h = vp.height - FIXED_HEADER_HEIGHT;
        if header_only || scroll_h <= 0.0 {
            sg::set_viewport(f, None);
            return;
        }
        sg::set_viewport(f, Some([vp.x, vp.y + FIXED_HEADER_HEIGHT, vp.width, scroll_h]));
        let layout = self.config_section_layout.borrow();
        for sec in layout.iter() {
            let (bx, by) = (sec.x, sec.y - self.scroll_y.get());
            if !(by + sec.height >= 0.0 && by < scroll_h) {
                continue;
            }
            sg::set_draw_layer(f, None, -10);
            sg::set_draw_color(f, 0.66, 0.66, 0.66);
            sg::draw_rect(f, bx, by, SECTION_WIDTH, sec.height);
            sg::set_draw_color(f, 0.1, 0.1, 0.1);
            sg::draw_rect(f, bx + 2.0, by + 2.0, SECTION_WIDTH - 4.0, sec.height - 4.0);
            sg::set_draw_layer(f, None, 0);
            let mut label = sec.name.clone();
            if sec.diff_count > 0 {
                label = format!("{label}  ({} diff)", sec.diff_count);
            }
            let lw = sg::draw_string_width(f, 14.0, "VAR", &label);
            sg::set_draw_color(f, 0.66, 0.66, 0.66);
            sg::draw_rect(f, bx + 6.0, by - 8.0, lw + 6.0, 18.0);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, bx + 7.0, by - 7.0, lw + 4.0, 16.0);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_string(f, bx + 9.0, by - 6.0, Some("LEFT"), 14.0, "VAR", &label);
            let mut row_y = by + SECTION_INNER_PAD;
            for row in &sec.rows {
                if row_y + ROW_HEIGHT >= 0.0 && row_y < scroll_h {
                    if row.is_diff {
                        sg::set_draw_layer(f, None, -5);
                        sg::set_draw_color(f, 0.18, 0.14, 0.08);
                        sg::draw_rect(f, bx + 3.0, row_y, SECTION_WIDTH - 6.0, ROW_HEIGHT);
                        sg::set_draw_layer(f, None, 0);
                    }
                    let v = row.info.var;
                    let text = v.label.as_deref().or(v.var.as_deref()).unwrap_or("");
                    let size = if sg::draw_string_width(f, 14.0, "VAR", text) > 228.0 {
                        12.0
                    } else {
                        14.0
                    };
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    sg::draw_string(f, bx + LABEL_OFFSET, row_y + 2.0, Some("LEFT"), size, "VAR", &format!("^7{text}"));
                }
                row_y += ROW_HEIGHT;
            }
        }
        if layout.is_empty() {
            sg::draw_string(
                f,
                10.0,
                -self.scroll_y.get(),
                Some("LEFT"),
                16.0,
                "VAR",
                &format!("{}No configuration options to display.", code("POSITIVE")),
            );
        }
        sg::set_viewport(f, None);
    }

    /// :1852-1858 `DrawControlList` over the header controls of the view, above the rows.
    pub(super) fn draw_config_header_controls(&self, f: &mut Frame<'_>, view_port: &Rect) {
        let c = self.c();
        let sel = self.host.sel();
        let list: [ControlRef; 5] = [
            c.copy_config_btn.clone(),
            c.config_toggle_btn.clone(),
            c.config_search_edit.clone(),
            c.config_primary_set_label.clone(),
            c.config_primary_set_select.clone(),
        ];
        for ctl in list {
            if ctl.is_shown(f) && ctl.has_draw() {
                // `self.selControl and self.selControl.hasFocus and self.selControl ~= control`
                let no_tip = sel
                    .as_ref()
                    .is_some_and(|s| s.base().has_focus.get() && !crate::control::same(&**s, &*ctl));
                ctl.draw(f, view_port, no_tip);
            }
        }
    }
}
