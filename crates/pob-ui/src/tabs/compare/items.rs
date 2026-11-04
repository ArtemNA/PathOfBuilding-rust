//! `CompareTab:DrawItems`, `DrawItemExpanded` (CompareTab.lua:3495-4044) and the helpers of `Classes/TradeHelpers.lua` they call (:71-99, :401-608). The Buy button is the network.

use std::collections::HashMap;
use std::rc::Rc;

use pob_calc::env::{CalcOverride, OverrideNode};
use pob_gfx::{Frame, Gfx};
use pob_items::item::{Item, ModLine};
use pob_lua::Repl;
use pob_lua::format::Arg;

use super::layout::*;
use super::power::JewelSlot;
use super::{CompareEntry, CompareTab};
use crate::colors::color_code;
use crate::control::{Control, Rect};
use crate::host::{InputEvent, InputEvents, ipairs};
use crate::item_tooltip::format_mod_line;
use crate::modules::main::{main, not_ported};
use crate::sg;
use crate::undo::UndoHost;

const ITEM_BOX_W: f64 = 310.0;
const ITEM_BOX_H: f64 = 20.0;
const COPY_BTN_W: f64 = 60.0;
const EQUIP_BTN_W: f64 = 60.0;
const BUY_BTN_W: f64 = 60.0;
const COPY_BTN_H: f64 = 18.0;
const COMPARE_COL_GAP: f64 = 48.0;

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

fn fmt(f: &str, v: f64) -> String {
    pob_lua::format::format(f, &[Arg::Num(v)]).unwrap_or_else(|e| panic!("{e}"))
}

/// `getRarityColor` (TradeHelpers.lua:71-78).
fn rarity_color(item: Option<&Rc<Item>>) -> String {
    item.and_then(|i| color_code(&i.rarity)).unwrap_or_else(|| "^7".to_string())
}

/// `modLineTemplate` (:82-85).
fn template(line: &str) -> String {
    String::from_utf8_lossy(
        &pob_lua::gsub(line.as_bytes(), b"%-?[%d]+%.?[%d]*", Repl::Str(b"#"), None)
            .expect("pattern")
            .0,
    )
    .into_owned()
}

/// The number of a capture.
fn number_at(line: &str, c: Option<pob_lua::Capture>) -> Option<f64> {
    pob_lua::tonumber(std::str::from_utf8(c?.bytes(line.as_bytes())?).ok()?)
}

/// `modLineValue` (:91-99).
fn line_value(line: &str) -> Option<f64> {
    if let Ok(Some(caps)) = pob_lua::match_(line.as_bytes(), b"(%-?%d+%.?%d*) to (%-?%d+%.?%d*)", None)
        && let (Some(a), Some(b)) = (number_at(line, caps.get(0)), number_at(line, caps.get(1)))
    {
        return Some((a + b) / 2.0);
    }
    let caps = pob_lua::match_(line.as_bytes(), b"(%-?[%d]+%.?[%d]*)", None).ok().flatten()?;
    number_at(line, caps.get(0))
}

fn mod_lists(item: &Item) -> [&Vec<ModLine>; 5] {
    [
        &item.enchant_mod_lines,
        &item.scourge_mod_lines,
        &item.implicit_mod_lines,
        &item.explicit_mod_lines,
        &item.crucible_mod_lines,
    ]
}

type ModMap = HashMap<String, (String, Option<f64>)>;

/// `buildModMap` (:401-416).
fn build_mod_map(item: Option<&Rc<Item>>) -> ModMap {
    let mut m = HashMap::new();
    let Some(item) = item else { return m };
    for list in mod_lists(item) {
        for ml in list {
            if item.check_mod_line_variant(ml) && format_mod_line(ml, false).is_some() {
                m.insert(template(&ml.line), (ml.line.clone(), line_value(&ml.line)));
            }
        }
    }
    m
}

/// `getSlotDiffLabel` (:421-434).
fn diff_label(p: Option<&Rc<Item>>, c: Option<&Rc<Item>>) -> String {
    match (p, c) {
        (None, None) => "^8(both empty)".to_string(),
        (Some(a), Some(b)) if a.name == b.name => format!("{}(match)", code("POSITIVE")),
        (None, _) => format!("{}(missing)", code("NEGATIVE")),
        (_, None) => format!("{}(extra)", code("TIP")),
        _ => format!("{}(different)", code("WARNING")),
    }
}

struct Buttons {
    b1: bool,
    b2: bool,
    b3: bool,
    b2x: f64,
    y: f64,
}

/// `drawCopyButtons` (:440-496).
fn draw_copy_buttons(f: &mut Frame<'_>, cx: f64, cy: f64, start_x: f64, y: f64, slot_missing: bool) -> Buttons {
    let h = COPY_BTN_H;
    let btn3 = start_x;
    let btn1 = btn3 + BUY_BTN_W + 4.0;
    let btn2 = btn1 + COPY_BTN_W + 4.0;
    let left = sg::is_key_down(f, "LEFTBUTTON");
    let draw = |f: &mut Frame<'_>, x: f64, w: f64, hover: bool, label: &str| {
        if hover {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        sg::draw_rect(f, x, y, w, h);
        if hover && left {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        } else if hover {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        sg::draw_rect(f, x + 1.0, y + 1.0, w - 2.0, h - 2.0);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(f, x + w / 2.0, y + 1.0, Some("CENTER_X"), 14.0, "VAR", label);
    };
    let inside = |x: f64, w: f64| cx >= x && cx < x + w && cy >= y && cy < y + h;
    let b3 = inside(btn3, BUY_BTN_W);
    draw(f, btn3, BUY_BTN_W, b3, "^7Buy");
    let b1 = inside(btn1, COPY_BTN_W);
    draw(f, btn1, COPY_BTN_W, b1, "^7Copy");
    let b2 = if slot_missing {
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(
            f,
            btn2 + EQUIP_BTN_W / 2.0,
            y + 1.0,
            Some("CENTER_X"),
            14.0,
            "VAR",
            "^xBBBBBBMissing slot",
        );
        false
    } else {
        let b = inside(btn2, EQUIP_BTN_W);
        draw(f, btn2, EQUIP_BTN_W, b, "^7Equip");
        b
    };
    Buttons { b1, b2, b3, b2x: btn2, y }
}

/// `fitItemName` (:499-514): byte lengths, as `name:sub`.
fn fit_item_name(g: &Gfx, color: &str, name: &str, max_w: f64) -> String {
    let display = format!("{color}{name}");
    if sg::draw_string_width(g, 16.0, "VAR", &display) <= max_w {
        return display;
    }
    let sub = |n: usize| String::from_utf8_lossy(&name.as_bytes()[..n]).into_owned();
    let (mut lo, mut hi) = (0, name.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if sg::draw_string_width(g, 16.0, "VAR", &format!("{color}{}...", sub(mid))) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    format!("{color}{}...", sub(lo))
}

/// The `Data` number of a table.
fn dnum(t: &pob_items::item::DataTable, k: &str) -> Option<f64> {
    t.num(k)
}

/// `DrawItemExpanded` (:3495-3666): the height, or in measure mode the widest line.
#[allow(clippy::collapsible_if)]
fn draw_item_expanded(f: &mut Frame<'_>, item: Option<&Rc<Item>>, x: f64, start_y: f64, other: Option<&ModMap>, measure: bool) -> f64 {
    let line_height = 16.0;
    let font = 14.0;
    let mut draw_y = start_y;
    let mut max_w: f64 = 0.0;
    let mut emit = |f: &mut Frame<'_>, ly: f64, fs: f64, s: &str| {
        if measure {
            max_w = max_w.max(sg::draw_string_width(f, fs, "VAR", s));
        } else {
            sg::draw_string(f, x, ly, Some("LEFT"), fs, "VAR", s);
        }
    };
    let Some(item) = item else {
        emit(f, draw_y, font, "^8(empty)");
        return if measure { max_w } else { line_height };
    };
    let magic = code("MAGIC");
    let label = |name: &str| format!("^x7F7F7F{name}: {magic}");
    emit(f, draw_y, 16.0, &format!("{}{}", rarity_color(Some(item)), item.name));
    draw_y += 18.0;
    if let Some(base) = item.base {
        if base.weapon.is_some() {
            if let Some(w) = item.weapon_data.as_ref().and_then(|d| d.get(&pob_items::item::NumKey(1.0))) {
                for (k, l) in [
                    ("PhysicalDPS", "Phys DPS"),
                    ("ElementalDPS", "Ele DPS"),
                    ("ChaosDPS", "Chaos DPS"),
                    ("TotalDPS", "Total DPS"),
                ] {
                    if let Some(v) = dnum(w, k) {
                        emit(f, draw_y, font, &fmt(&format!("{}%.1f", label(l)), v));
                        draw_y += line_height;
                    }
                }
                emit(
                    f,
                    draw_y,
                    font,
                    &fmt(&format!("{}%.2f%%", label("Crit")), dnum(w, "CritChance").unwrap_or(0.0)),
                );
                draw_y += line_height;
                emit(
                    f,
                    draw_y,
                    font,
                    &fmt(&format!("{}%.2f", label("APS")), dnum(w, "AttackRate").unwrap_or(0.0)),
                );
                draw_y += line_height;
            }
        } else if base.armour.is_some() {
            if let Some(a) = &item.armour_data {
                for (k, l, pct) in [
                    ("Armour", "Armour", ""),
                    ("Evasion", "Evasion", ""),
                    ("EnergyShield", "ES", ""),
                    ("Ward", "Ward", ""),
                    ("BlockChance", "Block", "%%"),
                ] {
                    if let Some(v) = dnum(a, k).filter(|v| *v > 0.0) {
                        emit(f, draw_y, font, &fmt(&format!("{}%d{pct}", label(l)), v));
                        draw_y += line_height;
                    }
                }
            }
        } else if base.flask.is_some() {
            if let Some(d) = &item.flask_data {
                let duration = dnum(d, "duration");
                for (k, l) in [("lifeTotal", "Life"), ("manaTotal", "Mana")] {
                    if let Some(v) = dnum(d, k) {
                        let s = pob_lua::format::format(
                            &format!("{}%d ^x7F7F7F(%.1fs)", label(l)),
                            &[Arg::Num(v), Arg::Num(duration.unwrap_or(0.0))],
                        )
                        .expect("format");
                        emit(f, draw_y, font, &s);
                        draw_y += line_height;
                    }
                }
                if dnum(d, "lifeTotal").is_none()
                    && dnum(d, "manaTotal").is_none()
                    && let Some(v) = duration
                {
                    emit(f, draw_y, font, &fmt(&format!("{}%.2fs", label("Duration")), v));
                    draw_y += line_height;
                }
                if let (Some(u), Some(m)) = (dnum(d, "chargesUsed"), dnum(d, "chargesMax")) {
                    let s = pob_lua::format::format(&format!("{}%d/%d", label("Charges")), &[Arg::Num(u), Arg::Num(m)]).expect("format");
                    emit(f, draw_y, font, &s);
                    draw_y += line_height;
                }
                for ml in &item.buff_mod_lines {
                    let c = if ml.flag("extra") { code("UNSUPPORTED") } else { magic.clone() };
                    emit(f, draw_y, font, &format!("{c}{}", ml.line));
                    draw_y += line_height;
                }
            }
        }
        if let Some(q) = item.quality.filter(|q| *q > 0.0)
            && base.weapon.is_none()
            && base.armour.is_none()
            && base.flask.is_none()
        {
            emit(f, draw_y, font, &fmt(&format!("{}+%d%%", label("Quality")), q));
            draw_y += line_height;
        }
    }
    if draw_y > start_y + 18.0 {
        draw_y += 2.0;
    }
    for list in mod_lists(item) {
        let mut drew = false;
        for ml in list {
            if !item.check_mod_line_variant(ml) {
                continue;
            }
            let Some(mut formatted) = format_mod_line(ml, false) else {
                continue;
            };
            if let Some(map) = other {
                match map.get(&template(&ml.line)) {
                    None => formatted = format!("{}+ {formatted}", code("POSITIVE")),
                    Some((line, value)) if *line != ml.line => {
                        let mine = line_value(&ml.line);
                        if let (Some(a), Some(b)) = (mine, *value) {
                            if a > b {
                                formatted = format!("{}> {formatted}", code("POSITIVE"));
                            } else if a < b {
                                formatted = format!("{}< {formatted}", code("NEGATIVE"));
                            }
                        }
                    }
                    Some(_) => {}
                }
            }
            emit(f, draw_y, font, &formatted);
            draw_y += line_height;
            drew = true;
        }
        if drew {
            draw_y += 2.0;
        }
    }
    let neg = code("NEGATIVE");
    for (on, l) in [(item.corrupted, "Corrupted"), (item.split, "Split"), (item.mirrored, "Mirrored")] {
        if on {
            emit(f, draw_y, font, &format!("{neg}{l}"));
            draw_y += line_height;
        }
    }
    if measure { max_w } else { draw_y - start_y }
}

#[derive(Default)]
struct Hover {
    item: Option<(Rc<Item>, bool)>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    equip: Option<(Rc<Item>, String, f64, f64)>,
    clicked_copy: Option<String>,
    clicked_equip: Option<String>,
    clicked_buy: Option<String>,
}

/// `processSlotButtons` (:3819-3843).
fn process_buttons(hover: &mut Hover, b: &Buttons, c_item: Option<&Rc<Item>>, copy: &str, equip: &str, input_events: &mut InputEvents) {
    if b.b2
        && let Some(i) = c_item
    {
        hover.equip = Some((i.clone(), equip.to_string(), b.b2x, b.y));
    }
    if c_item.is_some() {
        let ids: Vec<usize> = ipairs(input_events)
            .filter(|(_, e)| matches!(e, InputEvent::KeyUp { key, .. } if key == "LEFTBUTTON"))
            .map(|(i, _)| i)
            .collect();
        for id in ids {
            if b.b1 {
                hover.clicked_copy = Some(copy.to_string());
                input_events[id] = None;
            } else if b.b2 {
                hover.clicked_equip = Some(equip.to_string());
                input_events[id] = None;
            } else if b.b3 {
                hover.clicked_buy = Some(equip.to_string());
                input_events[id] = None;
            }
        }
    }
}

impl CompareTab {
    /// `CopyCompareItemToPrimary` (:1444-1464).
    fn copy_item_to_primary(&self, g: &mut Gfx, slot: &str, entry: &CompareEntry, and_use: bool) {
        let item = {
            let eng = entry.b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            e.item_by_id(e.slot_sel_item(slot))
        };
        let Some(item) = item else { return };
        let mut new_item = Item::parse_with(&item.raw, None, false);
        new_item.normalise_quality();
        let p = self.primary();
        let tab = p.items_tab.borrow().clone().expect("itemsTab");
        let id = tab.add_item(g, &new_item, true, None);
        if and_use && let Some(s) = tab.slots.borrow().get(slot) {
            s.set_sel_item_id(id);
        }
        tab.populate_slots(g);
        tab.add_undo_state(false);
        p.build_flag.set(true);
    }

    /// :1792-1833: the check box and the item set drop-downs, before the view.
    pub(super) fn layout_items_header(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>) {
        let c = self.c();
        c.items_expanded_check
            .base
            .x
            .set(vp.x + 10.0 + c.items_expanded_check.label_width.get());
        c.items_expanded_check.base.y.set(vp.y + 8.0);
        let col_width = self.items_col_width.get().unwrap_or_else(|| (vp.width / 2.0).floor().max(700.0));
        let label_w = sg::draw_string_width(f, 16.0, "VAR", "^7Item set:") + 4.0;
        let off = -c.items_h_scroll_bar.offset.get();
        let (build_y, row_y) = (vp.y + 30.0, vp.y + 52.0);
        let x = vp.x + off + 10.0;
        c.primary_item_set_label.base.x.set(x);
        c.primary_item_set_label.base.y.set(row_y + 2.0);
        c.primary_item_set_select.base.x.set(x + label_w);
        c.primary_item_set_select.base.y.set(row_y);
        let x2 = vp.x + off + col_width + 10.0;
        c.compare_item_set_label2.base.x.set(x2);
        c.compare_item_set_label2.base.y.set(row_y + 2.0);
        c.compare_item_set_select2.base.x.set(x2 + label_w);
        c.compare_item_set_select2.base.y.set(row_y);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        let primary = self.primary();
        let name = Self::short_build_name(primary.build_name.borrow().as_deref());
        sg::draw_string(f, x, build_y, Some("LEFT"), 18.0, "VAR", &format!("{}{name}", code("POSITIVE")));
        sg::draw_string(
            f,
            x2,
            build_y,
            Some("LEFT"),
            18.0,
            "VAR",
            &format!("{}{}", code("WARNING"), entry.label),
        );
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, vp.x + 4.0, vp.y + ITEMS_CHECKBOX_OFFSET - 2.0, vp.width - 8.0, 2.0);
        for (b, d) in [(&primary, &c.primary_item_set_select), (&entry.b, &c.compare_item_set_select2)] {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let ids = e
                .item_sets()
                .iter()
                .map(|s| (s.id, s.title.clone().unwrap_or_else(|| "Default".into())))
                .collect();
            super::populate(ids, e.active_item_set_id(), d, f);
        }
    }

    /// `DrawItems` (:3695-4044).
    #[allow(clippy::too_many_lines)]
    pub(super) fn draw_items(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>, input_events: &mut InputEvents) {
        let primary = self.primary();
        let mut base_slots: Vec<String> = [
            "Weapon 1",
            "Weapon 2",
            "Weapon 1 Swap",
            "Weapon 2 Swap",
            "Helmet",
            "Body Armour",
            "Gloves",
            "Boots",
            "Amulet",
            "Ring 1",
            "Ring 2",
            "Belt",
            "Flask 1",
            "Flask 2",
            "Flask 3",
            "Flask 4",
            "Flask 5",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        if self.should_show_ring3(entry) {
            base_slots.insert(9, "Ring 3".to_string());
        }
        self.add_abyss_sockets(f, entry, &mut base_slots, false);
        let primary_has_ring3 = {
            let eng = primary.engine.borrow();
            let db = eng.as_ref().expect("engine").result().env.player.borrow().mod_db.clone();
            matches!(db.borrow().flag(None, &["AdditionalRingSlot"]), Ok(Some(true)))
        };
        let jewels: Vec<JewelSlot> = self.jewel_comparison_slots(entry);
        let slot_item = |b: &crate::modules::build::BuildMode, s: &str| {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            e.item_by_id(e.slot_sel_item(s))
        };
        let rows: Vec<(String, Option<Rc<Item>>, Option<Rc<Item>>)> = base_slots
            .iter()
            .map(|s| (s.clone(), slot_item(&primary, s), slot_item(&entry.b, s)))
            .collect();
        let mut max_label_w: f64 = 0.0;
        for s in &base_slots {
            max_label_w = max_label_w.max(sg::draw_string_width(f, 16.0, "VAR", &format!("^7{s}:")));
        }
        for j in &jewels {
            max_label_w = max_label_w.max(sg::draw_string_width(f, 16.0, "VAR", &format!("^7{}:", j.label)));
        }
        max_label_w += 2.0;
        let expanded = self.items_expanded_mode.get();
        let primary_content_w = if expanded {
            let mut widest: f64 = 0.0;
            for (_, p, _) in &rows {
                if p.is_some() {
                    widest = widest.max(draw_item_expanded(f, p.as_ref(), 0.0, 0.0, None, true));
                }
            }
            for j in &jewels {
                if j.p_item.is_some() {
                    widest = widest.max(draw_item_expanded(f, j.p_item.as_ref(), 0.0, 0.0, None, true));
                }
            }
            let label_row = 10.0 + sg::draw_string_width(f, 16.0, "VAR", &format!("^7{}:", base_slots[0]));
            (20.0 + widest + 10.0).max(label_row)
        } else {
            let mut max_diff: f64 = 0.0;
            for (_, p, c) in &rows {
                max_diff = max_diff.max(sg::draw_string_width(f, 14.0, "VAR", &diff_label(p.as_ref(), c.as_ref())));
            }
            for j in &jewels {
                max_diff = max_diff.max(sg::draw_string_width(
                    f,
                    14.0,
                    "VAR",
                    &diff_label(j.p_item.as_ref(), j.c_item.as_ref()),
                ));
            }
            10.0 + max_label_w + 4.0 + ITEM_BOX_W + 6.0 + max_diff
        };
        let col_width = primary_content_w + COMPARE_COL_GAP;
        let content_width = col_width * 2.0;
        let needs_h = content_width > vp.width;
        self.items_col_width.set(Some(col_width));
        let c = self.c();
        let h = &c.items_h_scroll_bar;
        h.base.x.set(vp.x);
        h.base.y.set(vp.y + vp.height - ITEMS_H_SCROLL_BAR_HEIGHT);
        h.base.width.set(vp.width);
        h.set_content_dimension(f, content_width, vp.width);
        let bottom = if needs_h { ITEMS_H_SCROLL_BAR_HEIGHT } else { 0.0 };
        let scroll_view_h = vp.height - ITEMS_CHECKBOX_OFFSET - bottom;
        sg::set_viewport(f, Some([vp.x, vp.y + ITEMS_CHECKBOX_OFFSET, vp.width, scroll_view_h]));
        let mut draw_y = 4.0 - self.scroll_y.get();
        let off_x = -h.offset.get();
        let (cx, cy) = f.get_cursor_pos();
        let (cx, cy) = (cx - vp.x, cy - (vp.y + ITEMS_CHECKBOX_OFFSET));
        let mut hover = Hover::default();
        let ptab = primary.items_tab.borrow().clone().expect("itemsTab");
        // `drawSlotEntry` (:3847-3913)
        let mut draw_entry = |f: &mut Frame<'_>,
                              draw_y: &mut f64,
                              label: &str,
                              p: Option<&Rc<Item>>,
                              ci: Option<&Rc<Item>>,
                              copy: &str,
                              equip: &str,
                              p_warn: &str,
                              c_warn: &str,
                              missing: bool,
                              input_events: &mut InputEvents| {
            if expanded {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                let text = format!("^7{label}:{p_warn}");
                sg::draw_string(f, off_x + 10.0, *draw_y, Some("LEFT"), 16.0, "VAR", &text);
                let end = sg::draw_string_width(f, 16.0, "VAR", &text);
                sg::draw_string(
                    f,
                    off_x + 10.0 + end + 8.0,
                    *draw_y + 2.0,
                    Some("LEFT"),
                    14.0,
                    "VAR",
                    &diff_label(p, ci),
                );
                if ci.is_some() {
                    let b = draw_copy_buttons(f, cx, cy, off_x + content_width - 214.0, *draw_y + 21.0, missing);
                    process_buttons(&mut hover, &b, ci, copy, equip, input_events);
                }
                *draw_y += 20.0;
                let (pm, cm) = (build_mod_map(p), build_mod_map(ci));
                let start = *draw_y;
                let lh = draw_item_expanded(f, p, off_x + 20.0, *draw_y, Some(&cm), false);
                let rh = draw_item_expanded(f, ci, off_x + col_width + 20.0, *draw_y, Some(&pm), false);
                sg::set_draw_color(f, 0.25, 0.25, 0.25);
                let m = lh.max(rh);
                sg::draw_rect(f, off_x + col_width, start, 1.0, m);
                *draw_y += m + 6.0;
            } else {
                // `drawCompactSlotRow` (TradeHelpers.lua:523-608)
                let node_id = ptab.slots.borrow().get(equip).and_then(|s| s.node_id);
                let y = *draw_y;
                let mut p_name = p.map_or_else(|| "(empty)".to_string(), |i| i.name.clone());
                let mut c_name = ci.map_or_else(|| "(empty)".to_string(), |i| i.name.clone());
                p_name.push_str(p_warn);
                c_name.push_str(c_warn);
                let label_x = 10.0 + off_x;
                let p_box = label_x + max_label_w + 4.0;
                let c_box = col_width + 10.0 + off_x;
                let diff_x = p_box + ITEM_BOX_W + 6.0;
                let p_hover = p.is_some() && cx >= p_box && cx < p_box + ITEM_BOX_W && cy >= y && cy < y + ITEM_BOX_H;
                let c_hover = ci.is_some() && cx >= c_box && cx < c_box + ITEM_BOX_W && cy >= y && cy < y + ITEM_BOX_H;
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                let text = format!("^7{label}:");
                sg::draw_string(f, label_x, y + 2.0, Some("LEFT"), 16.0, "VAR", &text);
                if node_id.is_some() {
                    let w = sg::draw_string_width(f, 16.0, "VAR", &text);
                    sg::draw_rect(f, label_x, y + 18.0, w, 1.0);
                }
                let boxed = |f: &mut Frame<'_>, x: f64, hov: bool, s: &str| {
                    let gray = if hov { 0.5 } else { 0.33 };
                    sg::set_draw_color(f, gray, gray, gray);
                    sg::draw_rect(f, x, y, ITEM_BOX_W, ITEM_BOX_H);
                    sg::set_draw_color(f, 0.05, 0.05, 0.05);
                    sg::draw_rect(f, x + 1.0, y + 1.0, ITEM_BOX_W - 2.0, ITEM_BOX_H - 2.0);
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    let t = fit_item_name(
                        f,
                        &s[..s.find('\u{0}').unwrap_or(0)],
                        &s[s.find('\u{0}').map_or(0, |i| i + 1)..],
                        ITEM_BOX_W - 8.0,
                    );
                    sg::draw_string(f, x + 4.0, y + 2.0, Some("LEFT"), 16.0, "VAR", &t);
                };
                boxed(f, p_box, p_hover, &format!("{}\u{0}{p_name}", rarity_color(p)));
                sg::draw_string(f, diff_x, y + 3.0, Some("LEFT"), 14.0, "VAR", &diff_label(p, ci));
                boxed(f, c_box, c_hover, &format!("{}\u{0}{c_name}", rarity_color(ci)));
                let buttons = ci
                    .is_some()
                    .then(|| draw_copy_buttons(f, cx, cy, c_box + ITEM_BOX_W + 6.0, y + 1.0, missing));
                // :3887-3901 the socket under the label
                if label_x <= cx
                    && cx <= label_x + max_label_w
                    && y < cy
                    && cy <= y + 20.0
                    && let Some(n) = node_id
                {
                    sg::set_viewport(f, None);
                    let size = 250.0;
                    ptab.draw_socket_viewer(f, n, vp.x + label_x, vp.y + ITEMS_CHECKBOX_OFFSET + y - size, size, size);
                    sg::set_viewport(f, Some([vp.x, vp.y + ITEMS_CHECKBOX_OFFSET, vp.width, scroll_view_h]));
                }
                if p_hover {
                    hover.item = p.map(|i| (i.clone(), true));
                    (hover.x, hover.y, hover.w, hover.h) = (p_box, y, ITEM_BOX_W, ITEM_BOX_H);
                } else if c_hover {
                    hover.item = ci.map(|i| (i.clone(), false));
                    (hover.x, hover.y, hover.w, hover.h) = (c_box, y, ITEM_BOX_W, ITEM_BOX_H);
                }
                if let Some(b) = &buttons {
                    process_buttons(&mut hover, b, ci, copy, equip, input_events);
                }
                *draw_y += 20.0;
            }
        };
        for (slot, p, ci) in &rows {
            let missing = slot == "Ring 3" && !primary_has_ring3;
            draw_entry(
                f,
                &mut draw_y,
                slot,
                p.as_ref(),
                ci.as_ref(),
                slot,
                slot,
                "",
                "",
                missing,
                input_events,
            );
        }
        // :3927-3953 the tree sets
        draw_y += 22.0;
        let abs_y = vp.y + ITEMS_CHECKBOX_OFFSET + draw_y;
        let label_w = sg::draw_string_width(f, 16.0, "VAR", "^7Tree set:") + 4.0;
        c.primary_tree_set_label.base.x.set(vp.x + off_x + 10.0);
        c.primary_tree_set_label.base.y.set(abs_y + 2.0);
        c.primary_tree_set_select.base.x.set(vp.x + off_x + 10.0 + label_w);
        c.primary_tree_set_select.base.y.set(abs_y);
        c.compare_tree_set_label.base.x.set(vp.x + off_x + col_width + 10.0);
        c.compare_tree_set_label.base.y.set(abs_y + 2.0);
        c.compare_tree_set_select.base.x.set(vp.x + off_x + col_width + 10.0 + label_w);
        c.compare_tree_set_select.base.y.set(abs_y);
        for (b, d) in [(&primary, &c.primary_tree_set_select), (&entry.b, &c.compare_tree_set_select)] {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            *d.list.borrow_mut() = super::spec_list_drop(e);
            d.sel_index.set(e.active_spec());
        }
        draw_y += 24.0;
        if !jewels.is_empty() {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_string(f, off_x + 10.0, draw_y, Some("LEFT"), 16.0, "VAR", "^7-- Jewels --");
            draw_y += 20.0;
            let warn = format!("{}  (tree missing allocated node)", code("WARNING"));
            for j in &jewels {
                let pw = if j.p_item.is_some() && !j.p_node_allocated {
                    warn.as_str()
                } else {
                    ""
                };
                let cw = if j.c_item.is_some() && !j.c_node_allocated {
                    warn.as_str()
                } else {
                    ""
                };
                draw_entry(
                    f,
                    &mut draw_y,
                    &j.label,
                    j.p_item.as_ref(),
                    j.c_item.as_ref(),
                    &j.slot_name,
                    &j.slot_name,
                    pw,
                    cw,
                    false,
                    input_events,
                );
            }
        }
        if let Some(s) = hover.clicked_copy.take() {
            self.copy_item_to_primary(f, &s, entry, false);
        } else if let Some(s) = hover.clicked_equip.take() {
            self.copy_item_to_primary(f, &s, entry, true);
        }
        if hover.clicked_buy.is_some() {
            not_ported(f, "Classes/CompareTab.lua:3980 buySimilar.openPopup (trade, network)");
        }
        sg::set_viewport(f, None);
        let max_tip = 600f64.min((vp.width - 24.0).max(260.0));
        let no_popup = main().popups.borrow().is_empty();
        if no_popup && let Some((item, is_primary)) = &hover.item {
            let tab = if *is_primary {
                ptab.clone()
            } else {
                entry.b.items_tab.borrow().clone().expect("itemsTab")
            };
            let mut t = self.item_tooltip.borrow_mut();
            t.clear(false);
            tab.add_tooltip_width(&mut t, f, item, None, false, Some(max_tip));
            sg::set_draw_layer(f, None, 100);
            t.draw(
                f,
                vp.x + hover.x,
                vp.y + ITEMS_CHECKBOX_OFFSET + hover.y,
                Some((hover.w, hover.h)),
                vp,
            );
            sg::set_draw_layer(f, None, 0);
        }
        if no_popup
            && hover.item.is_none()
            && let Some((item, slot, bx, by)) = &hover.equip
        {
            let mut t = self.item_tooltip.borrow_mut();
            t.clear(false);
            t.max_width = Some(max_tip);
            let mut new_item = Item::parse_with(&item.raw, None, false);
            new_item.normalise_quality();
            let (sel, slot_label, node) = {
                let slots = ptab.slots.borrow();
                let s = slots.get(slot.as_str());
                (
                    s.map(|s| s.sel_item_id.get()),
                    s.map(|s| s.label.borrow().clone()),
                    s.and_then(|s| s.node_id),
                )
            };
            let peng = primary.engine.borrow();
            let pe = peng.as_ref().expect("engine");
            let sel_item = sel.and_then(|i| pe.item_by_id(i));
            let spec = pe.spec_rc();
            let add_nodes = node
                .filter(|n| spec.node(*n).is_some_and(|x| !x.alloc))
                .and_then(|n| spec.node_idx(n))
                .map(|i| vec![OverrideNode::Spec(i)]);
            let ov = CalcOverride {
                rep_slot_name: Some(slot.clone()),
                rep_item: Some(Rc::new(new_item)),
                add_nodes,
                ..CalcOverride::default()
            };
            let label = slot_label.unwrap_or_else(|| slot.clone());
            let header = match sel_item {
                Some(si) => format!(
                    "^7Equipping this item in {label} will give you:\n(replacing {}{}^7)",
                    color_code(&si.rarity).unwrap_or_else(|| "^7".into()),
                    si.name
                ),
                None => format!("^7Equipping this item in {label} will give you:"),
            };
            let lines = pe.compare_fresh_lines(ov, &header).unwrap_or_else(|e| panic!("{e}"));
            if lines.is_empty() {
                t.add_line(f, 14.0, Some(&header));
                t.add_line(f, 14.0, Some("^7No changes."));
            } else {
                for l in &lines {
                    t.add_line(f, 14.0, Some(l));
                }
            }
            sg::set_draw_layer(f, None, 100);
            t.draw(f, vp.x + bx, vp.y + ITEMS_CHECKBOX_OFFSET + by, Some((vp.width, COPY_BTN_H)), vp);
            sg::set_draw_layer(f, None, 0);
        }
        self.items_total_content_height.set(draw_y + self.scroll_y.get() + 36.0);
        sg::set_viewport(f, None);
        let _ = c.items_expanded_check.is_shown(f);
    }
}
