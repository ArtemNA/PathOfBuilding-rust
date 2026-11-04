//! The Tree view of `Classes/CompareTab.lua`: its controls (:735-918), `CopyCompareSpecToPrimary` (:1336-1373), `LayoutTreeView` (:1885-2028), `DrawTree` (:3403-3486).

use std::rc::{Rc, Weak};

use pob_gfx::{Frame, Gfx};

use super::layout::*;
use super::{CompareEntry, CompareTab};
use crate::colors::color_code;
use crate::control::{ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::host::InputEvents;
use crate::modules::main::{BareControl, not_ported};
use crate::sg;

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

/// `self.treeLayout`.
#[derive(Clone, Copy)]
pub(super) struct TreeLayout {
    overlay: bool,
    half_width: f64,
    footer_y: f64,
}

pub struct TreeControls {
    pub overlay_check: Rc<CheckBoxControl>,
    pub overlay_search: Rc<EditControl>,
    pub left_footer: Rc<BareControl>,
    pub right_footer: Rc<BareControl>,
    pub left_spec: Rc<DropDownControl>,
    pub left_version: Rc<DropDownControl>,
    pub left_search: Rc<EditControl>,
    pub right_spec: Rc<DropDownControl>,
    pub right_version: Rc<DropDownControl>,
    pub copy_spec: Rc<ButtonControl>,
    pub copy_spec_use: Rc<ButtonControl>,
    pub right_search: Rc<EditControl>,
}

fn version_list() -> Vec<DropItem> {
    pob_data::tree::tree_versions()
        .expect("GameVersions")
        .tree_versions
        .iter()
        .map(|v| {
            let mut i = DropItem::table(&v.display);
            i.data = Some(Rc::new(v.ver.clone()));
            i
        })
        .collect()
}

impl CompareTab {
    /// :735-918.
    #[allow(clippy::too_many_lines)]
    pub(super) fn init_tree_controls(&self, g: &mut Gfx) -> TreeControls {
        let w: Weak<CompareTab> = self.me.clone();
        let t = move || w.upgrade().expect("compareTab");
        let tt = t.clone();
        let footer_shown = move |_: &Gfx| {
            let s = tt();
            *s.view_mode.borrow() == "TREE" && s.active_compare().is_some()
        };
        let tt = t.clone();
        let side_shown = move |_: &Gfx| {
            let s = tt();
            *s.view_mode.borrow() == "TREE" && s.active_compare().is_some() && !s.tree_overlay_mode.get()
        };
        let tt = t.clone();
        let overlay_shown = move |_: &Gfx| {
            let s = tt();
            *s.view_mode.borrow() == "TREE" && s.active_compare().is_some() && s.tree_overlay_mode.get()
        };
        let tt = t.clone();
        let overlay_check = CheckBoxControl::new(
            g,
            None,
            [0.0, 0.0, 20.0],
            Some("Overlay comparison"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                let s = tt();
                s.tree_overlay_mode.set(state);
                s.tree_search_needs_sync.set(true);
            })),
            None,
            true,
        );
        overlay_check.base.shown.set_fn(footer_shown.clone());
        let search = |g: &Gfx, w: f64, a, primary: bool, tt: Rc<dyn Fn() -> Rc<CompareTab>>| {
            EditControl::new(
                g,
                a,
                rect(0.0, 0.0, w, 20.0),
                EditArgs {
                    init: Some(String::new()),
                    prompt: Some("Search".into()),
                    filter: Some("%c".into()),
                    limit: Some(100),
                    clearable: true,
                    change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                        let s = tt();
                        let b = if primary {
                            Some(s.primary())
                        } else {
                            s.active_compare().map(|e| e.b.clone())
                        };
                        if let Some(b) = b
                            && let Some(v) = b.tree_view.borrow_mut().as_mut()
                        {
                            v.search_str = buf.to_string();
                        }
                    })),
                    ..EditArgs::default()
                },
            )
        };
        let trc: Rc<dyn Fn() -> Rc<CompareTab>> = Rc::new(t.clone());
        let overlay_search = search(g, 300.0, None, true, trc.clone());
        overlay_search.base.shown.set_fn(overlay_shown);
        let left_footer = BareControl::new(None, rect(0.0, 0.0, 0.0, 20.0));
        left_footer.base.shown.set_fn(side_shown.clone());
        let right_footer = BareControl::new(None, rect(0.0, 0.0, 0.0, 20.0));
        right_footer.base.shown.set_fn(side_shown.clone());
        let tt = t.clone();
        let left_spec = DropDownControl::new(
            g,
            None,
            rect(0.0, 0.0, 180.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let p = tt().primary();
                let n = p.engine.borrow().as_ref().expect("engine").spec_list_labels().len();
                if index >= 1 && index <= n {
                    p.mod_flag.set(true);
                    p.tree_tab_made(g).set_active_spec(g, index);
                }
            })),
            None,
            false,
        );
        left_spec.base.shown.set_fn(footer_shown.clone());
        left_spec.max_dropped_width.set(500.0);
        left_spec.enable_dropped_width.set(true);
        let ls: ControlRef = left_spec.clone();
        let tt = t.clone();
        let left_version = DropDownControl::new(
            g,
            anchor("LEFT", &ls, "RIGHT"),
            rect(4.0, 0.0, 100.0, 20.0),
            version_list(),
            Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                let p = tt().primary();
                let ver = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned();
                let cur = p.engine.borrow().as_ref().expect("engine").spec_rc().tree_version.clone();
                if let Some(ver) = ver.filter(|x| *x != cur) {
                    crate::tabs::tree::open_version_convert_popup(g, &ver, true);
                }
            })),
            None,
            false,
        );
        left_version.base.shown.set_fn(footer_shown.clone());
        let lf: ControlRef = left_footer.clone();
        let left_search = search(g, 200.0, anchor("TOPLEFT", &lf, "TOPLEFT"), true, trc.clone());
        left_search.base.shown.set_fn(side_shown.clone());
        let tt = t.clone();
        let right_spec = DropDownControl::new(
            g,
            None,
            rect(0.0, 0.0, 180.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = tt().active_compare() {
                    e.set_active_spec(g, index);
                }
            })),
            None,
            false,
        );
        right_spec.base.shown.set_fn(footer_shown.clone());
        right_spec.max_dropped_width.set(500.0);
        right_spec.enable_dropped_width.set(true);
        let rs: ControlRef = right_spec.clone();
        let tt = t.clone();
        let right_version = DropDownControl::new(
            g,
            anchor("LEFT", &rs, "RIGHT"),
            rect(4.0, 0.0, 100.0, 20.0),
            version_list(),
            Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                let Some(e) = tt().active_compare() else { return };
                let ver = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned();
                let cur = e.b.engine.borrow().as_ref().expect("engine").spec_rc().tree_version.clone();
                if ver.is_some_and(|x| x != cur) {
                    // the popup converts `build.spec` of the primary build's tab in the port
                    not_ported(g, "Classes/CompareTab.lua:889 entry.treeTab:OpenVersionConvertPopup");
                }
            })),
            None,
            false,
        );
        right_version.base.shown.set_fn(footer_shown.clone());
        let rv: ControlRef = right_version.clone();
        let tt = t.clone();
        let copy_spec = ButtonControl::new(
            anchor("LEFT", &rv, "RIGHT"),
            rect(4.0, 0.0, 76.0, 20.0),
            "Copy tree".to_string(),
            move |g| {
                tt().copy_compare_spec_to_primary(g, false);
            },
        );
        copy_spec.base.shown.set_fn(footer_shown.clone());
        let tt = t.clone();
        let has_active = move |_: &Gfx| tt().active_compare().is_some();
        copy_spec.base.enabled.set_fn(has_active.clone());
        let cs: ControlRef = copy_spec.clone();
        let tt = t.clone();
        let copy_spec_use = ButtonControl::new(
            anchor("LEFT", &cs, "RIGHT"),
            rect(2.0, 0.0, 100.0, 20.0),
            "Copy and use".to_string(),
            move |g| {
                tt().copy_compare_spec_to_primary(g, true);
            },
        );
        copy_spec_use.base.shown.set_fn(footer_shown);
        copy_spec_use.base.enabled.set_fn(has_active);
        let rf: ControlRef = right_footer.clone();
        let right_search = search(g, 200.0, anchor("TOPLEFT", &rf, "TOPLEFT"), false, trc);
        right_search.base.shown.set_fn(side_shown);
        self.host.add("treeOverlayCheck", overlay_check.clone());
        self.host.add("overlayTreeSearch", overlay_search.clone());
        self.host.add("leftFooterAnchor", left_footer.clone());
        self.host.add("rightFooterAnchor", right_footer.clone());
        self.host.add("leftSpecSelect", left_spec.clone());
        self.host.add("leftVersionSelect", left_version.clone());
        self.host.add("leftTreeSearch", left_search.clone());
        self.host.add("rightSpecSelect", right_spec.clone());
        self.host.add("rightVersionSelect", right_version.clone());
        self.host.add("copySpecBtn", copy_spec.clone());
        self.host.add("copySpecUseBtn", copy_spec_use.clone());
        self.host.add("rightTreeSearch", right_search.clone());
        TreeControls {
            overlay_check,
            overlay_search,
            left_footer,
            right_footer,
            left_spec,
            left_version,
            left_search,
            right_spec,
            right_version,
            copy_spec,
            copy_spec_use,
            right_search,
        }
    }

    /// `CopyCompareSpecToPrimary` (:1336-1373): `new("PassiveSpec")`, `RestoreUndoState(sourceSpec:CreateUndoState())`, no jewels.
    fn copy_compare_spec_to_primary(&self, g: &mut Gfx, and_use: bool) {
        let Some(entry) = self.active_compare() else { return };
        let mut spec = {
            let eng = entry.b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            e.model().tree.specs[e.active_spec() - 1].clone()
        };
        spec.title = Some(format!("{} (Compared)", spec.title.as_deref().unwrap_or("Default")));
        spec.jewels = Default::default();
        let p = self.primary();
        let n = {
            let mut eng = p.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            e.push_decoded_spec(spec).unwrap_or_else(|err| panic!("{err}"));
            e.spec_list_labels().len()
        };
        if and_use {
            p.tree_tab_made(g).set_active_spec(g, n);
        }
        // :1364-1370 the spec drop-down of the items tab
        if let Some(items) = p.items_tab.borrow().as_ref()
            && let Some(sel) = items.spec_select.borrow().as_ref()
        {
            let eng = p.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let titles: Vec<DropItem> = e
                .model()
                .tree
                .specs
                .iter()
                .map(|s| DropItem::str(s.title.as_deref().unwrap_or("Default")))
                .collect();
            sel.set_list(g, Some(titles));
            sel.sel_index.set(e.active_spec());
        }
        p.build_flag.set(true);
    }

    /// `LayoutTreeView` (:1885-2028).
    pub(super) fn layout_tree_view(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>) -> TreeLayout {
        let c = self.c();
        let t = &c.tree;
        let footer_y = vp.y + vp.height - TREE_FOOTER_HEIGHT;
        let band = |f: &mut Frame<'_>, x: f64, w: f64, y: f64, h: f64, line_y: f64| {
            sg::set_draw_color(f, 0.05, 0.05, 0.05);
            sg::draw_rect(f, x, y, w, h);
            sg::set_draw_color(f, 0.85, 0.85, 0.85);
            sg::draw_rect(f, x, line_y, w, 2.0);
        };
        band(f, vp.x, vp.width, vp.y, TREE_HEADER_HEIGHT, vp.y + TREE_HEADER_HEIGHT - 2.0);
        let overlay = self.tree_overlay_mode.get();
        let half_width = (vp.width / 2.0).floor() - 2.0;
        let spec_width = if overlay {
            (vp.width * 0.25).floor().min(200.0)
        } else {
            (half_width * 0.55).floor().min(200.0)
        };
        if overlay {
            band(f, vp.x, vp.width, footer_y, TREE_FOOTER_HEIGHT, footer_y);
            t.overlay_search.base.x.set(vp.x + 4.0);
            t.overlay_search.base.y.set(footer_y + 4.0);
            t.overlay_search.base.width.set(vp.width - 8.0);
        } else {
            let right = vp.x + half_width + 4.0;
            sg::set_draw_color(f, 0.05, 0.05, 0.05);
            sg::draw_rect(f, vp.x, footer_y, half_width, TREE_FOOTER_HEIGHT);
            sg::draw_rect(f, right, footer_y, half_width, TREE_FOOTER_HEIGHT);
            sg::set_draw_color(f, 0.85, 0.85, 0.85);
            sg::draw_rect(f, vp.x, footer_y, half_width, 2.0);
            sg::draw_rect(f, right, footer_y, half_width, 2.0);
            t.left_footer.base.x.set(vp.x + 4.0);
            t.left_footer.base.y.set(footer_y + 4.0);
            t.left_search.base.width.set(half_width - 8.0);
            t.right_footer.base.x.set(right + 4.0);
            t.right_footer.base.y.set(footer_y + 4.0);
            t.right_search.base.width.set(half_width - 8.0);
        }
        t.left_spec.base.x.set(vp.x + 4.0);
        t.left_spec.base.y.set(vp.y + 22.0);
        t.left_spec.base.width.set(spec_width);
        t.right_spec.base.x.set(vp.x + (vp.width / 2.0).floor() + 4.0);
        t.right_spec.base.y.set(vp.y + 22.0);
        t.right_spec.base.width.set(spec_width);
        t.overlay_check.base.x.set(vp.x + TREE_OVERLAY_CHECK_X);
        t.overlay_check.base.y.set(vp.y + 48.0);
        let primary = self.primary();
        let versions = &pob_data::tree::tree_versions().expect("GameVersions").tree_versions;
        for (b, spec, version) in [
            (&primary, &t.left_spec, &t.left_version),
            (&entry.b, &t.right_spec, &t.right_version),
        ] {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            *spec.list.borrow_mut() = super::spec_list_drop(e);
            spec.sel_index.set(e.active_spec());
            let cur = e.spec_rc().tree_version.clone();
            if let Some(i) = versions.iter().position(|v| v.ver == cur) {
                version.sel_index.set(i + 1);
            }
        }
        primary.ensure_tree_view();
        entry.b.ensure_tree_view();
        if self.tree_search_needs_sync.replace(false) {
            let of =
                |b: &crate::modules::build::BuildMode| b.tree_view.borrow().as_ref().map_or_else(String::new, |v| v.search_str.clone());
            let (ps, cs) = (of(&primary), of(&entry.b));
            t.left_search.set_text(f, &ps, false);
            t.overlay_search.set_text(f, &ps, false);
            t.right_search.set_text(f, &cs, false);
        }
        TreeLayout {
            overlay,
            half_width,
            footer_y,
        }
    }

    /// `DrawTree` (:3403-3486) and the redraw of :1758-1777.
    pub(super) fn draw_tree(
        &self,
        f: &mut Frame<'_>,
        vp: &Rect,
        input_events: &mut InputEvents,
        entry: &Rc<CompareEntry>,
        layout: Option<TreeLayout>,
    ) {
        let primary = self.primary();
        let tree_h = vp.height - TREE_HEADER_HEIGHT - TREE_FOOTER_HEIGHT;
        primary.tree_tab_made(f);
        let top = vp.y + TREE_HEADER_HEIGHT;
        let Some(lay) = layout else {
            // :3405 `if not layout then return end`, and the labels of :1774-1777 at the old places
            sg::set_draw_layer(f, Some(1), 0);
            self.draw_tree_labels(f, vp, entry);
            return;
        };
        if lay.overlay {
            let spec = entry.b.engine.borrow().as_ref().expect("engine").spec_rc();
            sg::set_viewport(f, Some([vp.x, top, vp.width, tree_h]));
            sg::set_draw_layer(f, None, 0);
            primary.set_tree_cursor_offset((vp.x, top));
            primary.ensure_tree_view();
            if let Some(v) = primary.tree_view.borrow_mut().as_mut() {
                v.compare_owner = Some(Rc::downgrade(&entry.b));
            }
            primary.draw_tree_view(f, &Rect::new(0.0, 0.0, vp.width, tree_h), input_events, Some(spec));
            sg::set_viewport(f, None);
            primary.set_tree_cursor_offset((0.0, 0.0));
            if let Some(v) = primary.tree_view.borrow_mut().as_mut() {
                v.compare_spec = None;
                v.compare_owner = None;
            }
        } else {
            let half = lay.half_width;
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
            sg::draw_rect(f, vp.x + half, top, 4.0, vp.height - TREE_HEADER_HEIGHT);
            let (mx, _) = f.get_cursor_pos();
            let left_input = mx < vp.x + half + 2.0;
            let mut none: InputEvents = Vec::new();
            sg::set_viewport(f, Some([vp.x, top, half, tree_h]));
            sg::set_draw_layer(f, None, 0);
            primary.set_tree_cursor_offset((vp.x, top));
            let vp_half = Rect::new(0.0, 0.0, half, tree_h);
            primary.draw_tree_view(f, &vp_half, if left_input { &mut *input_events } else { &mut none }, None);
            sg::set_viewport(f, None);
            primary.set_tree_cursor_offset((0.0, 0.0));
            let right = vp.x + half + 4.0;
            sg::set_viewport(f, Some([right, top, half, tree_h]));
            sg::set_draw_layer(f, None, 0);
            entry.b.set_tree_cursor_offset((right, top));
            let mut none2: InputEvents = Vec::new();
            entry
                .b
                .draw_tree_view(f, &vp_half, if left_input { &mut none2 } else { &mut *input_events }, None);
            sg::set_viewport(f, None);
            entry.b.set_tree_cursor_offset((0.0, 0.0));
        }
        // :1758-1777
        sg::set_draw_layer(f, Some(1), 0);
        sg::set_draw_color(f, 0.05, 0.05, 0.05);
        sg::draw_rect(f, vp.x, vp.y, vp.width, TREE_HEADER_HEIGHT);
        sg::set_draw_color(f, 0.85, 0.85, 0.85);
        sg::draw_rect(f, vp.x, vp.y + TREE_HEADER_HEIGHT - 2.0, vp.width, 2.0);
        sg::set_draw_color(f, 0.05, 0.05, 0.05);
        sg::draw_rect(f, vp.x, lay.footer_y, vp.width, TREE_FOOTER_HEIGHT);
        sg::set_draw_color(f, 0.85, 0.85, 0.85);
        sg::draw_rect(f, vp.x, lay.footer_y, vp.width, 2.0);
        self.draw_tree_labels(f, vp, entry);
    }

    fn draw_tree_labels(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>) {
        let c = self.c();
        let primary = self.primary();
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        let name = Self::short_build_name(primary.build_name.borrow().as_deref());
        let (lx, rx) = (c.tree.left_spec.base.x.get(f), c.tree.right_spec.base.x.get(f));
        sg::draw_string(f, lx, vp.y + 4.0, Some("LEFT"), 18.0, "VAR", &format!("{}{name}", code("POSITIVE")));
        sg::draw_string(
            f,
            rx,
            vp.y + 4.0,
            Some("LEFT"),
            18.0,
            "VAR",
            &format!("{}{}", code("WARNING"), entry.label),
        );
    }
}
