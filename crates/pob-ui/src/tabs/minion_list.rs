//! `Classes/MinionListControl.lua` (`MinionListControl`, `SpawnListControl`), `Classes/MinionSearchListControl.lua` and
//! `buildMode:OpenSpectreLibrary()` (Build.lua:1690-2023), as at 5606cc64f.
//!
//! R13: `pairs(self.data.spectres)` (Build.lua:1695, :1706) and `pairs(data.worldAreas)` (MinionListControl.lua:148) follow the string hash of
//! the process in LuaJIT. Here they follow the data's order (`spectreIds`, `world-areas.json`); the gate gives the oracle process's order with
//! [`set_order_sample`]. It matters where `table.sort` meets equal names (the sort is not stable) and where two areas share a name.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_gfx::{Frame, Gfx, ImageHandle};
use pob_lua::format::{Arg, format};

use crate::colors::color_code;
use crate::control::{Control, ControlBase, ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{DragTarget, ListControl, ListHooks, SelfTarget};
use crate::modules::build::BuildMode;
use crate::modules::main::main;
use crate::sg;
use crate::tooltip::Tooltip;

thread_local! {
    static MINIONS: RefCell<Option<Rc<serde_json::Value>>> = const { RefCell::new(None) };
}

fn minions() -> Rc<serde_json::Value> {
    MINIONS.with_borrow_mut(|m| {
        m.get_or_insert_with(|| Rc::new(pob_data::read_json("minions.json").unwrap_or_else(|e| panic!("minions.json: {e}"))))
            .clone()
    })
}

fn minion_name(id: &str) -> String {
    minions()["minions"][id]["name"].as_str().unwrap_or("").to_string()
}

fn fmt1(spec: &str, v: f64) -> String {
    format(spec, &[Arg::Num(v)]).unwrap_or_else(|e| panic!("{e}"))
}

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

/// `tostring(n)` / `n .. ""`.
fn tos(n: f64) -> String {
    pob_lua::tostring_num(n)
}

/// The skills of `minion.skillList` that `self.data.skills` has: (name, `data.skillColorMap[color] or colorCodes.NORMAL`).
fn skills_of(id: &str) -> Vec<(String, String)> {
    let m = minions();
    let skills = pob_data::skills::skills();
    m["minions"][id]["skillList"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|s| s.as_str().and_then(|s| skills.get(s)))
                .map(|x| {
                    let c = crate::gem_tooltip::skill_color(x.color);
                    (x.name.clone(), if c.is_empty() { cc("NORMAL") } else { c })
                })
                .collect()
        })
        .unwrap_or_default()
}

thread_local! {
    /// [`set_order_sample`]: `pairs(data.spectres)` and `pairs(data.worldAreas)` of the oracle's process.
    static ORDER_SAMPLE: RefCell<Option<(Vec<String>, Vec<String>)>> = const { RefCell::new(None) };
    /// `self.lastSelectedMinion` of buildMode (Build.lua:1785): it outlives the popup, so it lives with the build mode object.
    static LAST_SELECTED: RefCell<Option<(Weak<BuildMode>, String)>> = const { RefCell::new(None) };
}

/// The gate: the order `pairs(self.data.spectres)` and `pairs(data.worldAreas)` take in the oracle's process (R13); `None` = the data's order.
pub fn set_order_sample(sample: Option<(Vec<String>, Vec<String>)>) {
    ORDER_SAMPLE.with_borrow_mut(|s| *s = sample);
}

/// `for id in pairs(self.data.spectres)`.
fn spectre_pairs() -> Vec<String> {
    if let Some(s) = ORDER_SAMPLE.with_borrow(|s| s.as_ref().map(|s| s.0.clone())) {
        return s;
    }
    minions()["spectreIds"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn last_selected() -> Option<String> {
    let b = Rc::downgrade(&main().build_mode);
    LAST_SELECTED.with_borrow(|l| l.as_ref().filter(|(w, _)| w.ptr_eq(&b)).map(|(_, id)| id.clone()))
}

fn set_last_selected(id: &str) {
    let b = Rc::downgrade(&main().build_mode);
    LAST_SELECTED.with_borrow_mut(|l| *l = Some((b, id.to_string())));
}

type OnSelect = Rc<RefCell<Option<Rc<dyn Fn(&mut Gfx, &String)>>>>;

struct MinionHooks {
    is_dest_side: bool,
    /// `controls.list.OnSelect` / `controls.source.OnSelect` (Build.lua:1832-1839).
    on_select: OnSelect,
}

impl ListHooks<String> for MinionHooks {
    /// MinionListControl.lua:45-50
    fn get_row_value(&self, _l: &ListControl<String>, _g: &Gfx, column: usize, _i: usize, id: &String) -> String {
        if column == 1 { minion_name(id) } else { String::new() }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// :52-103
    fn add_value_tooltip(&self, _l: &ListControl<String>, g: &Gfx, tooltip: &mut Tooltip, _i: usize, id: &String) {
        if !tooltip.check_for_update(id.clone()) {
            return;
        }
        let all = minions();
        let m = &all["minions"][id.as_str()];
        let num = |k: &str| m[k].as_f64();
        tooltip.center = true;
        tooltip.add_line_ex(
            g,
            20.0,
            Some(&format!("^7{}", m["name"].as_str().unwrap_or(""))),
            Some("FONTIN SC"),
            false,
            None,
        );
        tooltip.center = false;
        tooltip.add_separator(Some(2.0));
        tooltip.add_line(g, 14.0, Some(&fmt1("^7Life Multiplier: x%.2f", num("life").unwrap_or(0.0))));
        if let Some(v) = num("energyShield") {
            tooltip.add_line(g, 14.0, Some(&fmt1("^7Energy Shield: %d%% of base Life", v * 100.0)));
        }
        if let Some(v) = num("armour") {
            tooltip.add_line(g, 14.0, Some(&fmt1("^7Armour Multiplier: x%.2f", 1.0 + v)));
        }
        if let Some(v) = num("evasion") {
            tooltip.add_line(g, 14.0, Some(&fmt1("^7Evasion Multiplier: x%.2f", 1.0 + v)));
        }
        tooltip.add_line(
            g,
            14.0,
            Some(&format!(
                "^7Resistances: {}{} ^7/ {}{} ^7/ {}{} ^7/ {}{}",
                cc("FIRE"),
                fmt1("%d", num("fireResist").unwrap_or(0.0)),
                cc("COLD"),
                fmt1("%d", num("coldResist").unwrap_or(0.0)),
                cc("LIGHTNING"),
                fmt1("%d", num("lightningResist").unwrap_or(0.0)),
                cc("CHAOS"),
                fmt1("%d", num("chaosResist").unwrap_or(0.0)),
            )),
        );
        tooltip.add_line(g, 14.0, Some(&fmt1("^7Base Damage: x%.2f", num("damage").unwrap_or(0.0))));
        tooltip.add_line(
            g,
            14.0,
            Some(&fmt1("^7Base Attack Speed: %.2f", 1.0 / num("attackTime").unwrap_or(0.0))),
        );
        let speed = num("baseMovementSpeed").unwrap_or_else(|| {
            panic!("Classes/MinionListControl.lua:77: attempt to perform arithmetic on field 'baseMovementSpeed' (a nil value)")
        });
        tooltip.add_line(g, 14.0, Some(&fmt1("^7Base Movement Speed: %.2f", speed / 10.0)));
        let skill_list_len = m["skillList"].as_array().map_or(0, Vec::len);
        if skill_list_len > 0 {
            tooltip.add_separator(Some(10.0));
            for (name, color) in skills_of(id) {
                tooltip.add_line(g, 14.0, Some(&format!("^7Skill: {color}{name}")));
            }
        }
        tooltip.add_separator(Some(10.0));
        let spawn = m["spawnLocation"]
            .as_array()
            .unwrap_or_else(|| panic!("Classes/MinionListControl.lua:89: attempt to get length of field 'spawnLocation' (a nil value)"));
        if !spawn.is_empty() {
            // :91-94 "Print (Map) or (Act 7) in white, and map name in green."
            let colored: Vec<String> = spawn
                .iter()
                .map(|l| {
                    let location = l.as_str().unwrap_or("");
                    let caps = pob_lua::match_(location.as_bytes(), b"^(.-)%s*(%b())%s*$", None)
                        .unwrap_or_else(|e| panic!("Classes/MinionListControl.lua:92: {e:?}"));
                    match caps {
                        Some(c) => {
                            let s = |i: usize| {
                                c.get(i)
                                    .and_then(|x| x.bytes(location.as_bytes()))
                                    .map(|b| String::from_utf8_lossy(b).into_owned())
                                    .unwrap_or_default()
                            };
                            format!("{}{} ^7{}", cc("RELIC"), s(0), s(1))
                        }
                        None => format!("{}{location}", cc("RELIC")),
                    }
                })
                .collect();
            for (i, spawn) in colored.iter().enumerate() {
                if i == 0 {
                    tooltip.add_line(g, 14.0, Some(&format!("^7Spawn: {spawn}")));
                } else {
                    // :99 indented so all locations line up vertically
                    tooltip.add_line(g, 14.0, Some(&format!("^7            {spawn}")));
                }
            }
        }
    }

    /// :105-107
    fn get_drag_value(&self, _l: &ListControl<String>, _i: usize, v: &String) -> (String, String) {
        ("MinionId".to_string(), v.clone())
    }

    /// :109-111
    fn can_receive_drag(&self, l: &ListControl<String>, drag_type: &str, value: &String) -> bool {
        drag_type == "MinionId" && !l.list.borrow().contains(value)
    }

    /// :113-115
    fn receive_drag(&self, l: &ListControl<String>, _g: &mut Gfx, _t: &str, value: &String) {
        let at = l.sel_drag_index.get().map_or(l.list.borrow().len(), |i| i - 1);
        l.list.borrow_mut().insert(at, value.clone());
    }

    /// :117-121
    fn on_sel_click(&self, _l: &ListControl<String>, _g: &mut Gfx, _i: usize, _v: &String, double_click: bool) -> Option<bool> {
        if double_click && self.is_dest_side {
            SOURCE.with_borrow(|s| s.upgrade()).expect("source").add_sel();
        }
        None
    }

    /// :123-129
    fn on_sel_delete(&self, l: &ListControl<String>, _g: &mut Gfx, index: usize, _v: &String) {
        if !self.is_dest_side {
            l.list.borrow_mut().remove(index - 1);
            l.sel_index.set(None);
            *l.sel_value.borrow_mut() = None;
        }
    }

    /// ListControl.lua:105-107 `self:OnSelect(self.selIndex, self.selValue)`
    fn on_select(&self, _l: &ListControl<String>, g: &mut Gfx, _index: usize, value: &String) {
        let f = self.on_select.borrow().clone();
        if let Some(f) = f {
            f(g, value);
        }
    }
}

/// `SpawnListControl` (MinionListControl.lua:131-174).
struct SpawnHooks;

impl ListHooks<String> for SpawnHooks {
    /// :141-143
    fn get_row_value(&self, _l: &ListControl<String>, _g: &Gfx, _column: usize, _i: usize, v: &String) -> String {
        v.clone()
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// :145-174 (`ConPrintf("YES")` at :156 writes to the console only)
    fn add_value_tooltip(&self, _l: &ListControl<String>, g: &Gfx, tooltip: &mut Tooltip, _i: usize, value: &String) {
        if !tooltip.check_for_update(value.clone()) {
            return;
        }
        let areas = pob_data::world_areas::world_areas();
        let sample = ORDER_SAMPLE.with_borrow(|s| s.as_ref().map(|s| s.1.clone()));
        let found = match sample {
            Some(order) => order.iter().filter_map(|id| areas.get(id)).find(|a| &a.name == value),
            None => areas.values().find(|a| &a.name == value),
        };
        if let Some(area) = found {
            tooltip.add_line(g, 18.0, Some(&area.name));
            if let Some(d) = &area.description {
                for line in d {
                    tooltip.add_line(g, 14.0, Some(&format!("{}\"{line}\"", cc("CURRENCY"))));
                }
            }
            if let Some(b) = area.boss_varieties.as_ref().filter(|b| !b.is_empty()) {
                tooltip.add_line(g, 14.0, Some(&format!("{}Bosses: ^7{}", cc("UNIQUE"), b.join(", "))));
            }
            tooltip.add_line(g, 14.0, Some(&format!("^7Area Level: {}", tos(area.level))));
            tooltip.add_separator(Some(10.0));
            tooltip.add_line(g, 14.0, Some("^7Spectres:"));
            for name in &area.monster_varieties {
                tooltip.add_line(g, 14.0, Some(&format!(" - {name}")));
            }
        } else {
            tooltip.add_line(g, 14.0, Some(&format!("^7World area not found: {value}")));
        }
    }
}

/// `MinionSearchListControl`: the list with `dest`.
pub struct MinionSource {
    pub list: Rc<ListControl<String>>,
    dest: Rc<ListControl<String>>,
    /// `self.unfilteredList`.
    unfiltered: RefCell<Vec<String>>,
    /// `self.list == self.unfilteredList` (the same table, MinionSearchListControl.lua:136): sorting the one sorts the other.
    aliased: Cell<bool>,
    search_text: RefCell<Option<Rc<EditControl>>>,
    search_mode: RefCell<Option<Rc<DropDownControl>>>,
    sort_mode: RefCell<Option<Rc<DropDownControl>>>,
}

thread_local! {
    static SOURCE: RefCell<Weak<MinionSource>> = const { RefCell::new(Weak::new()) };
    /// The popup's state, kept for the gate.
    static POPUP: RefCell<Option<Rc<Library>>> = const { RefCell::new(None) };
}

/// MinionSearchListControl.lua:57-70
const SORT_FIELDS: [(&str, bool); 12] = [
    ("name", true),
    ("totalHitPoints", false),
    ("life", false),
    ("energyShield", false),
    ("attackTime", true),
    ("damage", false),
    ("fireResist", false),
    ("coldResist", false),
    ("lightningResist", false),
    ("chaosResist", false),
    ("totalResist", false),
    ("baseMovementSpeed", false),
];

#[derive(PartialEq, PartialOrd)]
enum SortVal {
    Str(String),
    Num(f64),
}

impl MinionSource {
    /// MinionListControl.lua:39-43
    fn add_sel(&self) {
        let v = self.list.sel_value.borrow().clone();
        if let Some(v) = v
            && !self.dest.list.borrow().contains(&v)
        {
            self.dest.list.borrow_mut().push(v);
        }
    }

    /// MinionSearchListControl.lua:56-103
    fn sort_source_list(&self) {
        let index = self.sort_mode.borrow().as_ref().map_or(1, |d| d.sel_index.get());
        let Some(&(field, asc)) = SORT_FIELDS.get(index.wrapping_sub(1)) else {
            return;
        };
        let all = minions();
        let ms = &all["minions"];
        let ratio = *pob_data::constants::constants()
            .game_constants
            .get("EnergyShieldRatioOfLife")
            .expect("data.gameConstants.EnergyShieldRatioOfLife");
        let n = |m: &serde_json::Value, k: &str| m[k].as_f64();
        let value = |id: &String| -> Option<SortVal> {
            let m = &ms[id.as_str()];
            let life = || n(m, "life").expect("life");
            let es = || n(m, "energyShield").unwrap_or(0.0);
            Some(match field {
                "name" => SortVal::Str(m["name"].as_str()?.to_string()),
                "totalHitPoints" => SortVal::Num(life() + ((es() * ratio / 100.0) * life())),
                "energyShield" => SortVal::Num((es() * ratio / 100.0) * life()),
                "totalResist" => SortVal::Num(
                    n(m, "fireResist").unwrap_or(0.0)
                        + n(m, "coldResist").unwrap_or(0.0)
                        + n(m, "lightningResist").unwrap_or(0.0)
                        + n(m, "chaosResist").unwrap_or(0.0),
                ),
                k => SortVal::Num(n(m, k)?),
            })
        };
        let mut list = self.list.list.borrow().clone();
        pob_engine::lua::lua_sort(&mut list, &mut |a, b| {
            let (va, vb) = (value(a), value(b));
            let (Some(va), Some(vb)) = (va, vb) else {
                panic!("Classes/MinionSearchListControl.lua:93: attempt to compare nil with number ({field})")
            };
            if va == vb {
                minion_name(a).as_bytes() < minion_name(b).as_bytes()
            } else {
                match (&va, &vb) {
                    (SortVal::Str(x), SortVal::Str(y)) => {
                        if asc {
                            x.as_bytes() < y.as_bytes()
                        } else {
                            x.as_bytes() > y.as_bytes()
                        }
                    }
                    (SortVal::Num(x), SortVal::Num(y)) => {
                        if asc {
                            x < y
                        } else {
                            x > y
                        }
                    }
                    _ => unreachable!("one field, one type"),
                }
            }
        });
        if self.aliased.get() {
            self.unfiltered.borrow_mut().clone_from(&list);
        }
        *self.list.list.borrow_mut() = list;
    }

    /// MinionSearchListControl.lua:105-123
    fn does_entry_match(search: &str, id: &str, mode: usize) -> bool {
        if (mode == 1 || mode == 3) && match_or_pattern(&minion_name(id).to_ascii_lowercase(), search) == Some(true) {
            return true;
        }
        if mode == 2 || mode == 3 {
            for (s, _) in skills_of(id) {
                if match_or_pattern(&s.to_ascii_lowercase(), search) == Some(true) {
                    return true;
                }
            }
        }
        false
    }

    /// :125-139
    fn list_filter_changed(&self, g: &mut Gfx, buf: &str, mode: usize) {
        let mut search = String::new();
        for c in buf.to_ascii_lowercase().chars() {
            if "-.+[]$^%?*".contains(c) {
                search.push('%');
            }
            search.push(c);
        }
        if search.bytes().any(|c| !c.is_ascii_whitespace()) {
            let filtered: Vec<String> = self
                .unfiltered
                .borrow()
                .iter()
                .filter(|id| Self::does_entry_match(&search, id, mode))
                .cloned()
                .collect();
            *self.list.list.borrow_mut() = filtered;
            self.aliased.set(false);
            self.list.select_index(g, Some(1));
        } else {
            *self.list.list.borrow_mut() = self.unfiltered.borrow().clone();
            self.aliased.set(true);
        }
    }

    fn search_buf(&self) -> String {
        self.search_text.borrow().as_ref().expect("searchText").buf.borrow().clone()
    }

    fn search_mode_index(&self) -> usize {
        self.search_mode.borrow().as_ref().expect("searchModeDropDown").sel_index.get()
    }
}

/// `string:matchOrPattern(pattern)` (Common.lua:1038-1079); `None`: a pattern error (the `PCall`'s `err`).
pub fn match_or_pattern(s: &str, pattern: &str) -> Option<bool> {
    fn generate(pattern: &str) -> Option<Vec<String>> {
        // the subgroups: `pattern:gmatch("%b()")`
        let bytes = pattern.as_bytes();
        let mut groups: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for c in pob_lua::gmatch(bytes, b"%b()") {
            let c = c.ok()?;
            let sub = c.get(0).and_then(|x| x.bytes(bytes))?;
            let from = groups.last().map_or(0, |g| g.1.saturating_sub(1));
            let at = pattern[from..].find(std::str::from_utf8(sub).ok()?)? + from;
            let inner = std::str::from_utf8(&sub[1..sub.len() - 1]).ok()?;
            groups.push((at, at + sub.len(), generate(inner)?));
        }
        let first_end = groups.first().map_or(pattern.len(), |g| g.0);
        let mut generated = vec![pattern[..first_end].to_string()];
        for (i, (_, close, pats)) in groups.iter().enumerate() {
            let next = groups.get(i + 1).map_or(pattern.len(), |g| g.0);
            let regular = &pattern[*close..next];
            let mut tmp = Vec::new();
            for sp in &generated {
                for p in pats {
                    tmp.push(format!("{sp}{p}{regular}"));
                }
            }
            generated = tmp;
        }
        let mut out = Vec::new();
        for g in generated {
            for p in g.split('|').filter(|x| !x.is_empty()) {
                if !out.iter().any(|x: &String| x == p) {
                    out.push(p.to_string());
                }
            }
        }
        Some(out)
    }
    for p in generate(pattern)? {
        if pob_lua::match_(s.as_bytes(), p.as_bytes(), None).ok()?.is_some() {
            return Some(true);
        }
    }
    Some(false)
}

// ---- the stat boxes (Build.lua:1903-2020): LabelControls with their own Draw --------------------------------------------------

/// A `LabelControl` whose `Draw` the library replaces: placed as the label (its width is the label's), drawn by `draw`.
struct DrawnLabel {
    label: Rc<LabelControl>,
    /// `self.labelText` / `self.lifeValue` / ...
    value: RefCell<Option<String>>,
    draw: fn(&DrawnLabel, &mut Frame<'_>),
}

impl DrawnLabel {
    fn new(
        anchor: Option<crate::control::Anchor>,
        r: crate::control::RectProps,
        label: String,
        draw: fn(&DrawnLabel, &mut Frame<'_>),
    ) -> Rc<Self> {
        Rc::new(Self {
            label: LabelControl::new(anchor, r, label),
            value: RefCell::new(None),
            draw,
        })
    }
}

impl Control for DrawnLabel {
    fn base(&self) -> &ControlBase {
        &self.label.base
    }
    fn has_draw(&self) -> bool {
        true
    }
    fn draw(&self, f: &mut Frame<'_>, _vp: &Rect, _no_tooltip: bool) {
        (self.draw)(self, f);
    }
}

/// Build.lua:1876-1885 `controls.minionNameLabel.Draw`
fn draw_name_label(l: &DrawnLabel, f: &mut Frame<'_>) {
    let (x, y) = l.get_pos(f);
    sg::set_draw_color_str(f, &cc("RELIC"));
    sg::draw_rect(f, x - 78.0, y - 10.0, 245.0, 38.0);
    sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 1.0);
    sg::draw_rect(f, x - 76.0, y - 8.0, 241.0, 34.0);
    sg::set_draw_color(f, 1.0, 1.0, 1.0);
    let text = l.value.borrow().clone().unwrap_or_else(|| "Monster Stats".to_string());
    sg::draw_string(f, x + 45.0, y, Some("CENTER_X"), 18.0, "VAR BOLD", &text);
}

/// One stat box (Build.lua:1893-2017, the same body seven times): `title` drawn, `frame` the colour code of the frame, `value` below.
fn draw_stat_box(l: &DrawnLabel, f: &mut Frame<'_>, title: &str, frame: &str, box_width: f64) {
    let (x, y) = l.get_pos(f);
    let box_height = 50.0;
    let label_width = sg::draw_string_width(f, 16.0, "VAR BOLD", title);
    sg::set_draw_color_str(f, &cc(frame));
    sg::draw_rect(f, x + (label_width / 2.0) - (box_width / 2.0), y - 3.0, box_width, box_height);
    sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 1.0);
    sg::draw_rect(
        f,
        x + (label_width / 2.0) - (box_width / 2.0) + 2.0,
        y - 1.0,
        box_width - 4.0,
        box_height - 4.0,
    );
    sg::set_draw_color_str(f, &cc(frame));
    sg::draw_rect(f, x + (label_width / 2.0) - (box_width / 2.0), y + 16.0, box_width, 2.0);
    sg::set_draw_color(f, 1.0, 1.0, 1.0);
    sg::draw_string(f, x + (label_width / 2.0), y, Some("CENTER_X"), 16.0, "VAR BOLD", title);
    if let Some(v) = l.value.borrow().as_ref() {
        sg::draw_string(f, x + (label_width / 2.0), y + 24.0, Some("CENTER_X"), 16.0, "VAR", v);
    }
}

fn draw_life(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "LIFE", "LIFE", 120.0);
}
fn draw_es(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "ENERGY SHIELD", "ES", 120.0);
}
fn draw_armour(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "ARMOUR", "NORMAL", 120.0);
}
fn draw_evasion(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "EVASION", "EVASION", 120.0);
}
fn draw_block(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "BLOCK", "NORMAL", 120.0);
}
fn draw_resists(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "RESISTS", "DEFENCE", 120.0);
}
fn draw_movement(l: &DrawnLabel, f: &mut Frame<'_>) {
    draw_stat_box(l, f, "MOVEMENT SPEED", "DEFENCE", 244.0);
}

/// The library's state: the closures of `OpenSpectreLibrary` and the controls they reach.
struct Library {
    dest: Rc<ListControl<String>>,
    source: Rc<MinionSource>,
    /// `sourceList` (the table `buildSourceList` refills).
    source_list: RefCell<Vec<String>>,
    /// `beastList, humanoidList, eldritchList, constructList, demonList, undeadList` by category.
    by_category: Vec<(&'static str, Vec<String>)>,
    recommended: HashSet<String>,
    corpse: HashSet<String>,
    /// `monsterTypeSort` (Build.lua:1726-1735); an absent key is `nil`.
    monster_type_sort: RefCell<HashMap<&'static str, bool>>,
    checkboxes: RefCell<Vec<(&'static str, Rc<CheckBoxControl>)>>,
    stats: RefCell<Option<Stats>>,
}

struct Stats {
    name: Rc<DrawnLabel>,
    gem_level: Rc<EditControl>,
    life: Rc<DrawnLabel>,
    es: Rc<DrawnLabel>,
    armour: Rc<DrawnLabel>,
    evasion: Rc<DrawnLabel>,
    block: Rc<DrawnLabel>,
    resists: Rc<DrawnLabel>,
    movement: Rc<DrawnLabel>,
    spawn: Rc<ListControl<String>>,
}

const CATEGORIES: [&str; 6] = ["Beast", "Humanoid", "Eldritch", "Construct", "Demon", "Undead"];

impl Library {
    /// `buildSourceList(sourceList)` (Build.lua:1737-1780)
    fn build_source_list(&self) {
        let sort = self.monster_type_sort.borrow();
        let on = |k: &str| sort.get(k).copied().unwrap_or(false);
        let mut list = Vec::new();
        for (key, ids) in &self.by_category {
            if on(key) {
                list.extend(ids.iter().cloned());
            }
        }
        // :1758 "Apply 'recommended' filter if checkbox is off"
        if !on("recommendedList") {
            list.retain(|id| self.recommended.contains(id));
        }
        if on("corpseList") {
            list.retain(|id| self.corpse.contains(id));
        }
        *self.source_list.borrow_mut() = list;
    }

    /// `monsterTypeCheckboxChange(name)(state)` (Build.lua:1840-1848)
    fn checkbox_change(&self, g: &mut Gfx, name: &'static str, state: bool) {
        self.monster_type_sort.borrow_mut().insert(name, state);
        self.build_source_list();
        let s = &self.source;
        *s.unfiltered.borrow_mut() = self.source_list.borrow().clone();
        s.list_filter_changed(g, &s.search_buf(), s.search_mode_index());
        s.sort_source_list();
    }

    /// `UpdateMinionDisplay(selected)` (Build.lua:1784-1830)
    fn update_minion_display(&self, selected: &str) {
        set_last_selected(selected);
        let stats = self.stats.borrow();
        let st = stats.as_ref().expect("the stat controls");
        let all = minions();
        let minion = &all["minions"][selected];
        let num = |k: &str| minion[k].as_f64();
        let buf = st.gem_level.buf.borrow().clone();
        // `m_min(m_max(controls.minionGemLevel.buf, 1), 40)`: math.max takes a numeric string
        let level = pob_lua::tonumber(&buf)
            .unwrap_or_else(|| panic!("Modules/Build.lua:1787: bad argument #1 to 'max' (number expected, got string)"));
        let gem_level = level.clamp(1.0, 40.0);
        let skills = pob_data::skills::skills();
        let minion_level = skills
            .get("RaiseSpectre")
            .and_then(|s| s.level(gem_level))
            .and_then(|l| l.values.get(1).copied())
            .unwrap_or_else(|| panic!("Modules/Build.lua:1788: attempt to index field '?' (RaiseSpectre level {gem_level})"));
        let c = pob_data::constants::constants();
        let at = |t: &[f64]| {
            pob_data::constants::monster_table_at(t, minion_level.min(100.0))
                .unwrap_or_else(|| panic!("Modules/Build.lua:1789: monster table at level {minion_level} is nil"))
        };
        let base_life = at(&c.monster_life_table);
        let total_life = base_life * num("life").expect("life");
        let ratio = *c.game_constants.get("EnergyShieldRatioOfLife").expect("EnergyShieldRatioOfLife");
        let total_es = match num("energyShield") {
            Some(es) => total_life * (es * (ratio / 100.0)),
            None => 0.0,
        };
        let mut total_armour = at(&c.monster_armour_table);
        let mut total_evasion = at(&c.monster_evasion_table);
        if let Some(a) = num("armour") {
            total_armour *= a;
        }
        if let Some(e) = num("evasion") {
            total_evasion *= e;
        }
        // :1807-1818 the last BlockChance / SpellBlockChance mod of minion.modList
        let (mut block, mut spell_block) = (tos(0.0), tos(0.0));
        for m in minion["modList"].as_array().into_iter().flatten() {
            let value = || match &m["value"] {
                serde_json::Value::Number(n) => tos(n.as_f64().unwrap_or(0.0)),
                other => unimplemented!("Modules/Build.lua:1813: a BlockChance mod with the value {other}"),
            };
            match m["name"].as_str() {
                Some("BlockChance") => block = value(),
                Some("SpellBlockChance") => spell_block = value(),
                _ => {}
            }
        }
        let spawn: Vec<String> = minion["spawnLocation"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let speed = num("baseMovementSpeed")
            .unwrap_or_else(|| panic!("Modules/Build.lua:1825: attempt to perform arithmetic on field 'baseMovementSpeed' (a nil value)"));
        let set = |l: &DrawnLabel, v: String| *l.value.borrow_mut() = Some(v);
        set(&st.name, format!("^7{}", minion["name"].as_str().unwrap_or("")));
        set(&st.life, tos(total_life.floor()));
        set(&st.es, tos(total_es.floor()));
        set(&st.armour, tos(pob_lua::number::round(total_armour, None)));
        set(&st.block, format!("{block}% / {spell_block}%"));
        set(&st.evasion, tos(pob_lua::number::round(total_evasion, None)));
        *st.spawn.list.borrow_mut() = spawn;
        let r = |k: &str| tos(num(k).unwrap_or_else(|| panic!("Modules/Build.lua:1834: attempt to concatenate field '{k}' (a nil value)")));
        set(
            &st.resists,
            format!(
                "{}{}^7 / {}{}^7 / {}{}^7 / {}{}",
                cc("FIRE"),
                r("fireResist"),
                cc("COLD"),
                r("coldResist"),
                cc("LIGHTNING"),
                r("lightningResist"),
                cc("CHAOS"),
                r("chaosResist")
            ),
        );
        set(&st.movement, format!("{} m/s", tos(speed / 10.0)));
    }
}

/// `buildMode:OpenSpectreLibrary()` (Build.lua:1690-2023).
#[allow(clippy::too_many_lines)]
pub fn open_spectre_library(g: &mut Gfx) {
    let b = main().build_mode.clone();
    let dest_list = b.engine.borrow().as_ref().expect("engine").model().spectres.clone();
    let all = minions();
    let ms = &all["minions"];
    // :1694-1704 `sourceList` sorted by name and id: `buildSourceList` wipes it before anything reads it
    let pairs = spectre_pairs();
    let mut by_category: Vec<(&'static str, Vec<String>)> = CATEGORIES.iter().map(|c| (*c, Vec::new())).collect();
    let (mut recommended, mut corpse) = (HashSet::new(), HashSet::new());
    // :1706-1725
    for id in &pairs {
        let m = &ms[id.as_str()];
        if let Some(cat) = m["monsterCategory"].as_str()
            && let Some(l) = by_category.iter_mut().find(|(c, _)| *c == cat)
        {
            l.1.push(id.clone());
        }
        if m["itemisedCorpse"].as_bool() == Some(true) {
            corpse.insert(id.clone());
        }
        if m["extraFlags"]["recommendedSpectre"].as_bool() == Some(true) {
            recommended.insert(id.clone());
        }
    }
    // :1726-1735 (`Corpse = true` is never read: the check box writes `corpseList`)
    let monster_type_sort: HashMap<&'static str, bool> = CATEGORIES
        .iter()
        .map(|c| (*c, true))
        .chain([("recommendedSpectreList", false), ("Corpse", true)])
        .collect();

    let dest_on_select: OnSelect = Rc::new(RefCell::new(None));
    let list = ListControl::new(
        None,
        rect(-230.0, 40.0, 210.0, 270.0),
        16.0,
        Some("VERTICAL"),
        true,
        dest_list,
        false,
        Box::new(MinionHooks {
            is_dest_side: false,
            on_select: dest_on_select.clone(),
        }),
    );
    list.label.set(Some("^7Spectres in Build:".to_string()));
    let lr: ControlRef = list.clone();
    let l2 = Rc::downgrade(&list);
    let remove = ButtonControl::new(
        anchor("BOTTOMRIGHT", &lr, "TOPRIGHT"),
        rect(0.0, -2.0, 60.0, 18.0),
        "Remove".to_string(),
        move |g| {
            let l = l2.upgrade().expect("list");
            let v = l.sel_value.borrow().clone();
            if let (Some(i), Some(v)) = (l.sel_index.get(), v) {
                l.hooks.on_sel_delete(&l, g, i, &v);
            }
        },
    );
    let l2 = Rc::downgrade(&list);
    remove
        .base
        .enabled
        .set_fn(move |_| l2.upgrade().expect("list").sel_value.borrow().is_some());
    list.host.add("delete", remove);

    // MinionSearchListControl.lua:14-54: `self:MinionListControl(...)`, then `sortSourceList`, then `unfilteredList = copyTable(list)`
    let source_on_select: OnSelect = Rc::new(RefCell::new(None));
    let src = ListControl::new(
        None,
        rect(0.0, 80.0, 210.0, 230.0),
        16.0,
        Some("VERTICAL"),
        false,
        Vec::new(),
        false,
        Box::new(MinionHooks {
            is_dest_side: true,
            on_select: source_on_select.clone(),
        }),
    );
    src.label.set(Some("^7Available Spectres:".to_string()));
    let target: Rc<dyn DragTarget<String>> = Rc::new(SelfTarget(Rc::downgrade(&list)));
    src.set_drag_target_list(vec![target]);
    let source = Rc::new(MinionSource {
        list: src.clone(),
        dest: list.clone(),
        unfiltered: RefCell::new(Vec::new()),
        aliased: Cell::new(false),
        search_text: RefCell::new(None),
        search_mode: RefCell::new(None),
        sort_mode: RefCell::new(None),
    });
    let lib = Rc::new(Library {
        dest: list.clone(),
        source: source.clone(),
        source_list: RefCell::new(Vec::new()),
        by_category,
        recommended,
        corpse,
        monster_type_sort: RefCell::new(monster_type_sort),
        checkboxes: RefCell::new(Vec::new()),
        stats: RefCell::new(None),
    });
    // :1782 `buildSourceList(sourceList)`
    lib.build_source_list();
    *src.list.borrow_mut() = lib.source_list.borrow().clone();
    source.sort_source_list();
    *source.unfiltered.borrow_mut() = src.list.borrow().clone();
    SOURCE.with_borrow_mut(|s| *s = Rc::downgrade(&source));
    let sr: ControlRef = src.clone();
    let s2 = Rc::downgrade(&source);
    let add = ButtonControl::new(
        anchor("BOTTOMRIGHT", &sr, "TOPRIGHT"),
        // MinionListControl.lua:21 {0, -2, 60, 18}, then MinionSearchListControl.lua:48 `self.controls.add.y - 40`
        rect(0.0, -42.0, 60.0, 18.0),
        "Add".to_string(),
        move |_g| {
            s2.upgrade().expect("source").add_sel();
        },
    );
    let s2 = Rc::downgrade(&source);
    add.base.enabled.set_fn(move |_| {
        let s = s2.upgrade().expect("source");
        let v = s.list.sel_value.borrow().clone();
        v.is_some_and(|v| !s.dest.list.borrow().contains(&v))
    });
    src.host.add("add", add);
    let s2 = Rc::downgrade(&source);
    let search_text = EditControl::new(
        g,
        anchor("BOTTOMLEFT", &sr, "TOPLEFT"),
        rect(0.0, -2.0, 148.0, 18.0),
        EditArgs {
            init: Some(String::new()),
            prompt: Some("Search".into()),
            filter: Some("%c".into()),
            limit: Some(100),
            clearable: true,
            change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _| {
                let s = s2.upgrade().expect("source");
                let mode = s.search_mode_index();
                s.list_filter_changed(g, buf, mode);
                s.sort_source_list();
            })),
            ..Default::default()
        },
    );
    let tr: ControlRef = search_text.clone();
    let s2 = Rc::downgrade(&source);
    let search_mode = DropDownControl::new(
        g,
        anchor("LEFT", &tr, "RIGHT"),
        rect(2.0, 0.0, 60.0, 18.0),
        vec![DropItem::str("Names"), DropItem::str("Skills"), DropItem::str("Both")],
        Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
            let s = s2.upgrade().expect("source");
            let buf = s.search_buf();
            s.list_filter_changed(g, &buf, index);
            s.sort_source_list();
        })),
        None,
        false,
    );
    let mr: ControlRef = search_mode.clone();
    let s2 = Rc::downgrade(&source);
    let sort_mode = DropDownControl::new(
        g,
        anchor("BOTTOMRIGHT", &mr, "TOPRIGHT"),
        // `{0, -2, self.width, 18}`
        rect(0.0, -2.0, 210.0, 18.0),
        [
            "Sort by Names",
            "Sort by Life + ES",
            "Sort by Life",
            "Sort by Energy Shield",
            "Sort by Attack Speed",
            "Sort by Base Damage",
            "Sort by Fire Resistance",
            "Sort by Cold Resistance",
            "Sort by Lightning Resistance",
            "Sort by Chaos Resistance",
            "Sort by Total Resistance",
            "Sort by Movement Speed",
        ]
        .into_iter()
        .map(DropItem::str)
        .collect(),
        Some(Rc::new(move |_g: &mut Gfx, _index: usize, _v: &DropItem| {
            s2.upgrade().expect("source").sort_source_list();
        })),
        None,
        false,
    );
    *source.search_text.borrow_mut() = Some(search_text.clone());
    *source.search_mode.borrow_mut() = Some(search_mode.clone());
    *source.sort_mode.borrow_mut() = Some(sort_mode.clone());
    src.host.add("searchText", search_text.clone());
    src.host.add("searchModeDropDown", search_mode);
    src.host.add("sortModeDropDown", sort_mode);
    // MinionSearchListControl.lua:46
    src.label_position_offset.set((0.0, -40.0));

    let w = Rc::downgrade(&lib);
    *dest_on_select.borrow_mut() = Some(Rc::new(move |_g: &mut Gfx, v: &String| {
        w.upgrade().expect("library").update_minion_display(v);
    }));
    let w = Rc::downgrade(&lib);
    *source_on_select.borrow_mut() = Some(Rc::new(move |_g: &mut Gfx, v: &String| {
        w.upgrade().expect("library").update_minion_display(v);
    }));

    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert("list".into(), list.clone());
    controls.insert("source".into(), src.clone());
    // :1849-1866 the monster category check boxes, with their icons
    let root = crate::launch::launch().cfg.script_path.clone();
    let boxes: [(&'static str, f64, &str); 6] = [
        ("Beast", 0.0, "categorybeasts"),
        ("Humanoid", 37.0, "categoryhumanoid"),
        ("Eldritch", 74.0, "categoryeldritch"),
        ("Construct", 111.0, "categoryconstruct"),
        ("Demon", 148.0, "categorydemon"),
        ("Undead", 184.0, "categoryundead"),
    ];
    let mut checkboxes = Vec::new();
    let change = |name: &'static str| -> Rc<dyn Fn(&mut Gfx, bool)> {
        let w = Rc::downgrade(&lib);
        Rc::new(move |g: &mut Gfx, state: bool| w.upgrade().expect("library").checkbox_change(g, name, state))
    };
    for (name, x, file) in boxes {
        let icon = ImageHandle::new();
        let _ = icon.load(crate::src_cache::resolve(&root, &format!("Assets/monstercategory/{file}.png")), &[]);
        let checkbox = CheckBoxControl::new(
            g,
            anchor("TOPLEFT", &sr, "BOTTOMLEFT"),
            [x, 30.0, 26.0],
            Some(""),
            Some(change(name)),
            Some(name),
            true,
        );
        // :1862 `checkbox:SetCheckImage({ handle = monsterType.icon, 1 })`
        checkbox.set_check_image(Some((icon, 1)));
        checkbox.base.shown.set(true);
        controls.insert(format!("sortMonsterCheckbox{name}"), checkbox.clone());
        checkboxes.push((name, checkbox));
    }
    let show_all = CheckBoxControl::new(
        g,
        anchor("TOPLEFT", &sr, "BOTTOMLEFT"),
        [184.0, 2.0, 26.0],
        Some(""),
        Some(change("recommendedList")),
        Some("By default only recommended Spectres are shown.\nEnable this box to show all Spectres."),
        false,
    );
    let corpse_box = CheckBoxControl::new(
        g,
        anchor("TOPLEFT", &sr, "BOTTOMLEFT"),
        [89.0, 2.0, 26.0],
        Some(""),
        Some(change("corpseList")),
        Some("Hide Spectres found in Acts or Maps and only show\nItemised Corpses obtainable through trade or Rituals."),
        false,
    );
    checkboxes.push(("recommendedList", show_all.clone()));
    checkboxes.push(("corpseList", corpse_box.clone()));
    *lib.checkboxes.borrow_mut() = checkboxes;
    let cr: ControlRef = corpse_box.clone();
    let ar: ControlRef = show_all.clone();
    controls.insert("sortMonsterCheckboxShowAll".into(), show_all);
    controls.insert("sortMonsterCheckboxFilterCorpse".into(), corpse_box);
    controls.insert(
        "showOnlyCorpseLabel".into(),
        LabelControl::new(anchor("RIGHT", &cr, "LEFT"), rect(-5.0, 0.0, 0.0, 16.0), "Corpse Only:".to_string()),
    );
    controls.insert(
        "showAllLabel".into(),
        LabelControl::new(anchor("RIGHT", &ar, "LEFT"), rect(-5.0, 0.0, 0.0, 16.0), "Show All:".to_string()),
    );
    let l2 = list.clone();
    let save = ButtonControl::new(None, rect(-45.0, 420.0, 80.0, 20.0), "Save".to_string(), move |_g| {
        let b = main().build_mode.clone();
        b.engine
            .borrow_mut()
            .as_mut()
            .expect("engine")
            .set_spectre_list(l2.list.borrow().clone());
        b.mod_flag.set(true);
        b.build_flag.set(true);
        main().close_popup();
    });
    let save_ref: ControlRef = save.clone();
    controls.insert("save".into(), save);
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 420.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
    );
    controls.insert(
        "noteLine1".into(),
        LabelControl::new(
            anchor("TOP", &save_ref, "BOTTOM"),
            rect(45.0, -60.0, 0.0, 16.0),
            "^7Spectres in your Library must be assigned to an active".to_string(),
        ),
    );
    controls.insert(
        "noteLine2".into(),
        LabelControl::new(
            anchor("TOP", &save_ref, "BOTTOM"),
            rect(45.0, -42.0, 0.0, 16.0),
            "^7Raise Spectre gem for their buffs and curses to activate".to_string(),
        ),
    );
    let popup = main().open_popup(g, 720.0, 450.0, "Spectre Library", controls, None, None, None, None);
    // `spectrePopup:SelectControl(spectrePopup.controls.source.controls.searchText)`
    popup.host.select_control(g, Some(search_text));

    // :1874-2023 the controls added to the popup's table after `OpenPopup`
    let name = DrawnLabel::new(
        anchor("TOP", &sr, "TOP"),
        rect(230.0, -50.0, 0.0, 18.0),
        "Minion Stats".into(),
        draw_name_label,
    );
    let nr: ControlRef = name.clone();
    let gem_level_label = LabelControl::new(
        anchor("BOTTOM", &nr, "TOP"),
        rect(24.0, 271.0, 0.0, 16.0),
        "^7Gem Level:".to_string(),
    );
    let glr: ControlRef = gem_level_label.clone();
    let w = Rc::downgrade(&lib);
    let gem_level = EditControl::new(
        g,
        anchor("LEFT", &glr, "RIGHT"),
        rect(4.0, 0.0, 60.0, 20.0),
        EditArgs {
            init: Some("20".into()),
            prompt: None,
            filter: Some("%D".into()),
            limit: Some(3),
            change_func: Some(Rc::new(move |_g: &mut Gfx, _buf: &str, _| {
                if let Some(last) = last_selected() {
                    w.upgrade().expect("library").update_minion_display(&last);
                }
            })),
            ..Default::default()
        },
    );
    let life = DrawnLabel::new(
        anchor("TOP", &sr, "TOP"),
        rect(170.0, -9.0, 0.0, 16.0),
        format!("{}LIFE", cc("LIFE")),
        draw_life,
    );
    let es = DrawnLabel::new(
        anchor("TOP", &sr, "TOP"),
        rect(293.0, -9.0, 0.0, 16.0),
        format!("{}ENERGY SHIELD", cc("ES")),
        draw_es,
    );
    let lfr: ControlRef = life.clone();
    let esr: ControlRef = es.clone();
    let armour = DrawnLabel::new(
        anchor("TOP", &lfr, "TOP"),
        rect(0.0, 54.0, 0.0, 16.0),
        format!("{}ARMOUR", cc("ARMOUR")),
        draw_armour,
    );
    let evasion = DrawnLabel::new(
        anchor("TOP", &esr, "TOP"),
        rect(1.0, 54.0, 0.0, 16.0),
        format!("{}EVASION", cc("EVASION")),
        draw_evasion,
    );
    let arr: ControlRef = armour.clone();
    let evr: ControlRef = evasion.clone();
    let block = DrawnLabel::new(
        anchor("TOP", &arr, "TOP"),
        rect(1.0, 54.0, 0.0, 16.0),
        format!("{}BLOCK", cc("NORMAL")),
        draw_block,
    );
    let resists = DrawnLabel::new(
        anchor("TOP", &evr, "TOP"),
        rect(1.0, 54.0, 0.0, 16.0),
        "RESISTS".into(),
        draw_resists,
    );
    let blr: ControlRef = block.clone();
    let movement = DrawnLabel::new(
        anchor("TOP", &blr, "TOP"),
        rect(61.0, 54.0, 0.0, 16.0),
        "MOVEMENT SPEED".into(),
        draw_movement,
    );
    let mvr: ControlRef = movement.clone();
    let spawn = ListControl::new(
        anchor("TOP", &mvr, "TOP"),
        rect(2.0, 73.0, 244.0, 68.0),
        16.0,
        Some("VERTICAL"),
        false,
        Vec::new(),
        false,
        Box::new(SpawnHooks),
    );
    spawn.label.set(Some("^7Spawns:".to_string()));
    for (k, c) in [
        ("minionNameLabel", name.clone() as ControlRef),
        ("minionGemLevelLabel", gem_level_label),
        ("minionGemLevel", gem_level.clone()),
        ("lifeLabel", life.clone()),
        ("energyshieldLabel", es.clone()),
        ("armourLabel", armour.clone()),
        ("evasionLabel", evasion.clone()),
        ("blockLabel", block.clone()),
        ("resistsLabel", resists.clone()),
        ("movementSpeedLabel", movement.clone()),
        ("spawnLocations", spawn.clone()),
    ] {
        popup.host.add(k, c);
    }
    *lib.stats.borrow_mut() = Some(Stats {
        name,
        gem_level,
        life,
        es,
        armour,
        evasion,
        block,
        resists,
        movement,
        spawn,
    });
    POPUP.with_borrow_mut(|p| *p = Some(lib));
}

/// The gate: the state of the open library.
pub fn debug_library() -> Option<serde_json::Value> {
    POPUP.with_borrow(|p| {
        p.as_ref().map(|lib| {
            let s = &lib.source;
            let st = lib.stats.borrow();
            let st = st.as_ref().expect("stats");
            let v = |l: &DrawnLabel| l.value.borrow().clone();
            let boxes: serde_json::Map<String, serde_json::Value> = lib
                .checkboxes
                .borrow()
                .iter()
                .map(|(k, c)| ((*k).to_string(), serde_json::json!(c.state.get())))
                .collect();
            serde_json::json!({
                "dest": lib.dest.list.borrow().clone(),
                "source": s.list.list.borrow().len(),
                "sourceFirst": s.list.list.borrow().first().cloned(),
                "sel": s.list.sel_index.get(),
                "search": s.search_buf(),
                "sourceList": s.list.list.borrow().clone(),
                "unfiltered": s.unfiltered.borrow().len(),
                "selValue": s.list.sel_value.borrow().clone(),
                "searchMode": s.search_mode_index(),
                "sortMode": s.sort_mode.borrow().as_ref().map(|d| d.sel_index.get()),
                "boxes": boxes,
                "stats": {
                    "name": v(&st.name), "life": v(&st.life), "es": v(&st.es), "armour": v(&st.armour), "evasion": v(&st.evasion),
                    "block": v(&st.block), "resists": v(&st.resists), "movement": v(&st.movement),
                },
                "gemLevel": st.gem_level.buf.borrow().clone(),
                "spawn": st.spawn.list.borrow().clone(),
            })
        })
    })
}

/// The gate: a child of the open library's lists (`source`, `list`).
pub fn debug_child(parent: &str, child: &str) -> Option<ControlRef> {
    POPUP.with_borrow(|p| {
        p.as_ref().and_then(|lib| {
            let host = if parent == "source" {
                &lib.source.list.host
            } else {
                &lib.dest.host
            };
            host.get(child)
        })
    })
}
