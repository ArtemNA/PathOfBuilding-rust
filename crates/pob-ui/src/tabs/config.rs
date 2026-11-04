//! `Classes/ConfigTab.lua` (+ `CustomModBlockControl`), `Classes/ConfigSetListControl.lua`, `Modules/ConfigVisibility.lua` (`isShowAllExcluded`). The options are `data/config-options.json`
//! (Modules/ConfigOptions.lua), the set being edited is the engine's (`pob_engine::config_tab`): a callback writes the input and runs `BuildModList` at once, the recalculation
//! waits for `buildFlag` (Build.lua:1258), as in the Lua.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_build::InputValue;
use pob_data::config_options::{ConfigVar, config_options};
use pob_engine::config_tab::{ConfigGate, ConfigUndo};
use pob_engine::engine::Engine;
use pob_gfx::{Frame, Gfx};
use pob_lua::tostring_num;
use serde_json::{Value as Json, json};

use crate::colors::color_code;
use crate::control::{Anchor, Control, ControlBase, ControlRef, Prop, Rect, WeakControl, anchor, anchor_weak, rect, same};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl, SectionControl};
use crate::controls::dragger::ResizableEditControl;
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl, InactiveText};
use crate::controls::list::{ListControl, ListHooks};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::modules::build::BuildMode;
use crate::modules::main::{BareControl, main, sel_by_value};
use crate::sg;
use crate::tooltip::Tooltip;
use crate::undo::{UndoHandler, UndoHost};

/// ConfigVisibility.lua:10
const EXCLUDE_KEYWORDS: [&str; 6] = ["recently", "in the last", "in the past", "in last", "in past", "pvp"];

const BOSS_SKILLS_TIP: &str = "^7Used to fill in defaults for specific boss skills if the boss config is not set

Bosses' damage is modified by roll range configuration, defaulted at a 70% roll, at the normal monster level for your character level (capped at 85)
Fill in the exact damage numbers if more precision is needed";

fn truthy(v: Option<&InputValue>) -> bool {
    !matches!(v, None | Some(InputValue::Bool(false)))
}

fn lua_tostring(v: &InputValue) -> String {
    match v {
        InputValue::Num(n) => tostring_num(*n),
        InputValue::Str(s) => s.clone(),
        InputValue::Bool(b) => b.to_string(),
    }
}

fn strip(s: &str) -> String {
    pob_engine::display::strip_escapes(s)
}

/// What one `if*` key tests.
#[derive(Clone, Copy, PartialEq)]
enum GateKind {
    /// :396-400 `input[ifOption]`.
    Option,
    Env(ConfigGate),
}

struct Gate {
    kind: GateKind,
    opts: Vec<String>,
    /// `if implyCond(varData) then return true end` comes first.
    imply: bool,
}

fn gate_opts(j: &Json) -> Vec<String> {
    match j {
        Json::String(s) => vec![s.clone()],
        Json::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        other => panic!("ConfigOptions: an if* value {other}"),
    }
}

/// The `if*` keys of a var, in the order ConfigTab.lua:382-662 tests them (any order: all must pass).
fn gates_of(v: &ConfigVar) -> Vec<Gate> {
    let mut out = Vec::new();
    let transfigured = v.gates.get("includeTransfigured").is_some_and(|x| x.as_bool() == Some(true));
    for (k, val) in v.gates.iter() {
        let (kind, imply) = match k.as_str() {
            "ifOption" => (GateKind::Option, false),
            "ifCond" => (GateKind::Env(ConfigGate::Cond), true),
            "ifMinionCond" => (GateKind::Env(ConfigGate::MinionCond), true),
            "ifEnemyCond" => (GateKind::Env(ConfigGate::EnemyCond), true),
            "ifCondTrue" => (GateKind::Env(ConfigGate::CondTrue), false),
            "ifMult" => (GateKind::Env(ConfigGate::Mult), true),
            "ifEnemyMult" => (GateKind::Env(ConfigGate::EnemyMult), true),
            "ifStat" => (GateKind::Env(ConfigGate::Stat), true),
            "ifTagType" => (GateKind::Env(ConfigGate::TagType), true),
            "ifFlag" => (GateKind::Env(ConfigGate::Flag), false),
            "ifMod" => (GateKind::Env(ConfigGate::Mod), true),
            "ifSkill" if transfigured => (GateKind::Env(ConfigGate::SkillTransfigured), false),
            "ifSkill" => (GateKind::Env(ConfigGate::Skill), false),
            "ifSkillFlag" => (GateKind::Env(ConfigGate::SkillFlag), false),
            "ifSkillData" => (GateKind::Env(ConfigGate::SkillData), false),
            "includeTransfigured" | "implyCond" | "implyCondList" | "legacy" | "hideIfInvalid" => continue,
            other => unimplemented!("Classes/ConfigTab.lua:382-662: the {other} gate has no data in this upstream"),
        };
        out.push(Gate {
            kind,
            opts: gate_opts(val),
            imply,
        });
    }
    out
}

#[derive(Clone)]
enum Kind {
    Check(Rc<CheckBoxControl>),
    Edit(Rc<EditControl>),
    Drop(Rc<DropDownControl>),
    Bare(Rc<BareControl>),
}

impl Kind {
    fn control(&self) -> ControlRef {
        match self {
            Kind::Check(c) => c.clone(),
            Kind::Edit(c) => c.clone(),
            Kind::Drop(c) => c.clone(),
            Kind::Bare(c) => c.clone(),
        }
    }
}

/// One entry of `varList` with its control.
struct VarEntry {
    data: &'static ConfigVar,
    kind: Kind,
    section: usize,
    gates: Vec<Gate>,
    imply_cond: Vec<String>,
    /// `configVisibility.isShowAllExcluded(varData)`.
    excluded: bool,
    hide_if_invalid: bool,
    /// `StripEscapes(varData.label or ""):lower()`.
    label_lower: String,
    label_control: Option<Rc<LabelControl>>,
    /// `outputCache`, `outputCacheRevision` (:731-732).
    output_cache: RefCell<(u64, HashMap<String, Vec<String>>)>,
}

/// A `SectionControl` with the fields ConfigTab adds to it.
pub struct ConfigSection {
    pub control: Rc<SectionControl>,
    pub collapsed: Cell<bool>,
    col: f64,
    var_control_list: RefCell<Vec<ControlRef>>,
    pub toggle: Rc<ButtonControl>,
}

pub struct ConfigTab {
    pub anchor: Rc<BareControl>,
    pub host: ControlHost,
    pub undo: UndoHandler<ConfigUndo>,
    me: Weak<Self>,
    build: Weak<BuildMode>,
    pub sections: Vec<ConfigSection>,
    vars: Vec<VarEntry>,
    var_index: HashMap<&'static str, usize>,
    pub toggle_configs: Cell<bool>,
    pub search: Rc<EditControl>,
    pub set_select: Rc<DropDownControl>,
    pub set_manage: Rc<ButtonControl>,
    pub toggle_configs_button: Rc<ButtonControl>,
    pub scroll_bar: Rc<ScrollBarControl>,
    section_anchor: Rc<LabelControl>,
    custom_section: usize,
    custom_add: Rc<ButtonControl>,
    pub custom_blocks: RefCell<Vec<Rc<CustomModBlock>>>,
    /// Next `t_insert(self.controls, ...)` name.
    next_control: Cell<usize>,
    /// `searchMatch`'s string for `controls.search.buf`: the buffer and its or-patterns (`None` = no `%S`).
    search_cache: RefCell<(String, Option<Vec<String>>)>,
    /// `IsKeyDown("ALT")` of this frame, for `inactiveText` (it takes no host).
    alt_down: Cell<bool>,
    pub(super) pending_sel: RefCell<Vec<(String, usize)>>,
}

/// `string:matchOrPattern(pattern)`'s `generateOrPatterns` (Common.lua:1039-1070): the keys of its table (a Lua table: unordered, the test only asks for any).
fn generate_or_patterns(pattern: &str) -> Vec<String> {
    struct Sub {
        open: usize,
        close: usize,
        patterns: Vec<String>,
    }
    let bytes = pattern.as_bytes();
    let mut subs: Vec<Sub> = Vec::new();
    for caps in pob_lua::gmatch(bytes, b"%b()") {
        let Ok(c) = caps else { break };
        let Some(pob_lua::Capture::Str { start, end }) = c.get(0) else {
            continue;
        };
        let group = &pattern[start..end];
        // `pattern:find(subGroup, (subGroups[index] and subGroups[index].close or 1), true)`: index is past the last entry, so always from 1
        let open = pattern.find(group).expect("the group of this pattern");
        subs.push(Sub {
            open: open + 1,
            close: open + group.len(),
            patterns: generate_or_patterns(&group[1..group.len() - 1]),
        });
    }
    // `pattern:sub(1, (subGroups[1] and subGroups[1].open or 0) - 1)`: to the end without a group
    let sub = |from: usize, to: i64| crate::lua::sub(pattern, from as i64, to);
    let mut generated = vec![sub(1, subs.first().map_or(0, |s| s.open as i64) - 1)];
    for (i, s) in subs.iter().enumerate() {
        let next = sub(s.close + 1, subs.get(i + 1).map_or(0, |n| n.open as i64) - 1);
        let mut temp = Vec::new();
        for g in &generated {
            for p in &s.patterns {
                temp.push(format!("{g}{p}{next}"));
            }
        }
        generated = temp;
    }
    let mut out: Vec<String> = Vec::new();
    for g in &generated {
        for p in g.split('|') {
            if !p.is_empty() && !out.iter().any(|x| x == p) {
                out.push(p.to_string());
            }
        }
    }
    out
}

impl UndoHost for ConfigTab {
    type State = ConfigUndo;
    fn undo_handler(&self) -> &UndoHandler<Self::State> {
        &self.undo
    }
    fn create_undo_state(&self) -> Self::State {
        self.e(Engine::config_undo_state)
    }
    /// :1250-1268 up to the host's part: `UpdateControls` needs the host and runs in [`ConfigTab::undo_g`] / [`ConfigTab::redo_g`].
    fn restore_undo_state(&self, state: Self::State) {
        let b = self.build();
        let mut eng = b.engine.borrow_mut();
        let e = eng.as_mut().expect("engine");
        e.config_restore_undo(state);
        *self.pending_sel.borrow_mut() = e.config_build_mod_list().unwrap_or_else(|err| panic!("{err}"));
    }
}

fn drop_item(label: &str, val: InputValue) -> DropItem {
    let mut it = DropItem::table(label);
    it.data = Some(Rc::new(val) as Rc<dyn Any>);
    it
}

fn item_val(item: &DropItem) -> Option<InputValue> {
    item.data.as_ref().and_then(|d| d.downcast_ref::<InputValue>()).cloned()
}

fn scalar(s: &pob_data::wire::Scalar) -> InputValue {
    match s {
        pob_data::wire::Scalar::Num(n) => InputValue::Num(*n),
        pob_data::wire::Scalar::Str(t) => InputValue::Str(t.clone()),
        pob_data::wire::Scalar::Bool(b) => InputValue::Bool(*b),
    }
}

impl ConfigTab {
    pub fn build(&self) -> Rc<BuildMode> {
        self.build.upgrade().expect("buildMode")
    }

    fn e<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        let b = self.build();
        let eng = b.engine.borrow();
        f(eng.as_ref().expect("engine"))
    }

    fn e_mut<R>(&self, f: impl FnOnce(&mut Engine) -> R) -> R {
        let b = self.build();
        let mut eng = b.engine.borrow_mut();
        f(eng.as_mut().expect("engine"))
    }

    /// :133-832
    pub fn new(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<Self> {
        let tab = Rc::new_cyclic(|me: &Weak<Self>| {
            let host = ControlHost::new();
            let a = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
            let a_ref: ControlRef = a.clone();
            let section_anchor = LabelControl::new(anchor("TOPLEFT", &a_ref, "TOPLEFT"), rect(0.0, 20.0, 0.0, 0.0), String::new());
            let sa_ref: ControlRef = section_anchor.clone();
            let m = me.clone();
            let set_select = DropDownControl::new(
                g,
                anchor("TOPLEFT", &sa_ref, "TOPLEFT"),
                rect(76.0, -12.0, 210.0, 20.0),
                Vec::new(),
                Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                    let t = m.upgrade().expect("configTab");
                    let id = t.e(Engine::config_set_order)[index - 1];
                    t.set_active_config_set(g, id);
                    t.add_undo_state(false);
                })),
                None,
                false,
            );
            set_select.enable_dropped_width.set(true);
            let m = me.clone();
            set_select
                .base
                .enabled
                .set_fn(move |_| m.upgrade().expect("configTab").e(Engine::config_set_order).len() > 1);
            let ss_ref: ControlRef = set_select.clone();
            let set_label = LabelControl::new(
                anchor("RIGHT", &ss_ref, "LEFT"),
                rect(-2.0, 0.0, 0.0, 16.0),
                "^7Config set:".to_string(),
            );
            let m = me.clone();
            let set_manage = ButtonControl::new(
                anchor("LEFT", &ss_ref, "RIGHT"),
                rect(4.0, 0.0, 90.0, 20.0),
                "Manage...".to_string(),
                move |g| open_config_set_manage_popup(g, m.clone()),
            );
            let m = me.clone();
            let search = EditControl::new(
                g,
                anchor("TOPLEFT", &sa_ref, "TOPLEFT"),
                rect(8.0, 15.0, 360.0, 20.0),
                EditArgs {
                    init: Some(String::new()),
                    prompt: Some("Search".into()),
                    filter: Some("%c".into()),
                    limit: Some(100),
                    change_func: Some(Rc::new(move |g: &mut Gfx, _buf: &str, _ph: bool| {
                        m.upgrade().expect("configTab").update_controls(g);
                    })),
                    clearable: true,
                    ..EditArgs::default()
                },
            );
            let search_ref: ControlRef = search.clone();
            let m = me.clone();
            let m2 = me.clone();
            let toggle_configs_button = ButtonControl::new(
                anchor("LEFT", &search_ref, "RIGHT"),
                rect(10.0, 0.0, 200.0, 20.0),
                Prop::func(move |_| {
                    if m.upgrade().expect("configTab").toggle_configs.get() {
                        "Hide Ineligible Configurations"
                    } else {
                        "Show All Configurations"
                    }
                    .to_string()
                }),
                move |_g| {
                    let t = m2.upgrade().expect("configTab");
                    t.toggle_configs.set(!t.toggle_configs.get());
                },
            );
            // :260-813 `for _, varData in ipairs(varList)`
            let opts = config_options();
            let mut sections: Vec<ConfigSection> = Vec::new();
            let mut vars: Vec<VarEntry> = Vec::new();
            let mut var_index = HashMap::new();
            let mut n = 0usize;
            let next = |n: &mut usize| {
                *n += 1;
                format!("#{n}")
            };
            let mut custom_section = None;
            for sec in &opts.sections {
                let s_index = sections.len();
                let control = SectionControl::new(
                    anchor("TOPLEFT", &search_ref, "BOTTOMLEFT"),
                    rect(0.0, 0.0, 360.0, 0.0),
                    sec.section.clone(),
                );
                let m = me.clone();
                control.base.height.set_fn(move |g| {
                    let t = m.upgrade().expect("configTab");
                    if t.is_section_collapsed(&t.sections[s_index]) {
                        return 16.0;
                    }
                    let mut height: f64 = 20.0;
                    let list = t.sections[s_index].var_control_list.borrow().clone();
                    for c in list {
                        if c.is_shown(g) {
                            let (_, h) = c.get_size(g);
                            height += h.max(16.0) + 4.0;
                        }
                    }
                    height.max(32.0)
                });
                let sec_ref: ControlRef = control.clone();
                let m = me.clone();
                let m2 = me.clone();
                let toggle = ButtonControl::new(
                    anchor("TOPRIGHT", &sec_ref, "TOPRIGHT"),
                    rect(-6.0, -7.0, 16.0, 16.0),
                    Prop::func(move |_| {
                        if m.upgrade().expect("configTab").sections[s_index].collapsed.get() {
                            "+"
                        } else {
                            "-"
                        }
                        .to_string()
                    }),
                    move |_g| {
                        let s = &m2.upgrade().expect("configTab").sections[s_index];
                        s.collapsed.set(!s.collapsed.get());
                    },
                );
                host.add(&next(&mut n), control.clone());
                host.add(&next(&mut n), toggle.clone());
                if sec.section == "Custom Modifiers" {
                    custom_section = Some(s_index);
                }
                let mut list: Vec<ControlRef> = Vec::new();
                for v in opts.vars.iter().filter(|v| v.section == sec.section) {
                    let i = vars.len();
                    let kind = Self::make_control(g, me, &sec_ref, v, i);
                    let c = kind.control();
                    let m = me.clone();
                    c.base().shown.set_fn(move |_| m.upgrade().expect("configTab").control_shown(i));
                    let mut label_control = None;
                    if let Some(label) = &v.label
                        && v.ty.as_deref() != Some("check")
                    {
                        let h = if sg::draw_string_width(g, 14.0, "VAR", label) > 228.0 {
                            12.0
                        } else {
                            14.0
                        };
                        let l = LabelControl::new(anchor("RIGHT", &c, "LEFT"), rect(-4.0, 0.0, 0.0, h), format!("^7{label}"));
                        host.add(&next(&mut n), l.clone());
                        label_control = Some(l);
                    }
                    let hide_if_invalid = v.gates.contains_key("hideIfInvalid");
                    if !hide_if_invalid {
                        let inner = format!("^7{}", v.label.as_deref().unwrap_or(""));
                        if let Some(l) = &label_control {
                            let m = me.clone();
                            l.label.set_fn(move |_| {
                                let t = m.upgrade().expect("configTab");
                                if t.invalid_state(i) {
                                    format!("{}{}", color_code("NEGATIVE").expect("NEGATIVE"), strip(&inner))
                                } else {
                                    inner.clone()
                                }
                            });
                        }
                        if let (Kind::Check(cb), Some(label)) = (&kind, &v.label) {
                            let m = me.clone();
                            let inner = label.clone();
                            cb.label.set_fn(move |_| {
                                let t = m.upgrade().expect("configTab");
                                Some(if t.invalid_state(i) {
                                    format!("{}{}", color_code("NEGATIVE").expect("NEGATIVE"), strip(&inner))
                                } else {
                                    inner.clone()
                                })
                            });
                        }
                    }
                    Self::wire_tooltip(me, &kind, i, hide_if_invalid);
                    Self::wire_border(me, &kind, i);
                    if let Some(name) = &v.var {
                        var_index.insert(name.as_str(), i);
                    }
                    let label_text = v.label.as_deref().unwrap_or("");
                    let lower = label_text.to_ascii_lowercase();
                    let g_has = |k: &str| v.gates.contains_key(k);
                    let excluded = g_has("ifOption")
                        || g_has("ifSkill")
                        || g_has("ifSkillData")
                        || g_has("ifSkillFlag")
                        || g_has("legacy")
                        || (v.label.is_some() && EXCLUDE_KEYWORDS.iter().any(|k| lower.contains(k)));
                    let mut imply_cond = Vec::new();
                    if let Some(l) = v.gates.get("implyCondList") {
                        imply_cond.extend(gate_opts(l));
                    }
                    if let Some(l) = v.gates.get("implyCond") {
                        imply_cond.extend(gate_opts(l));
                    }
                    vars.push(VarEntry {
                        data: v,
                        kind: kind.clone(),
                        section: s_index,
                        gates: gates_of(v),
                        imply_cond,
                        excluded,
                        hide_if_invalid,
                        label_lower: strip(label_text).to_ascii_lowercase(),
                        label_control,
                        output_cache: RefCell::new((0, HashMap::new())),
                    });
                    host.add(&next(&mut n), c.clone());
                    list.push(c);
                }
                sections.push(ConfigSection {
                    control,
                    collapsed: Cell::new(false),
                    col: sec.col,
                    var_control_list: RefCell::new(list),
                    toggle,
                });
            }
            let custom_section = custom_section.expect("Custom Modifiers");
            // :814
            let scroll_bar = ScrollBarControl::new(
                anchor("TOPRIGHT", &a_ref, "TOPRIGHT"),
                rect(0.0, 0.0, 18.0, 0.0),
                Some(50.0),
                Some("VERTICAL"),
                true,
            );
            // :815-829
            let custom_ref: ControlRef = sections[custom_section].control.clone();
            let m = me.clone();
            let custom_add = ButtonControl::new(
                anchor("TOPLEFT", &custom_ref, "TOPLEFT"),
                rect(8.0, 0.0, 120.0, 20.0),
                "^7Add Mod Group".to_string(),
                move |g| {
                    let t = m.upgrade().expect("configTab");
                    let mut list = t.e(|e| e.config_state().custom_mods_list.clone());
                    list.push(pob_build::CustomModsBlockModel {
                        title: format!("Group {}", list.len() + 1),
                        enabled: true,
                        text: pob_build::BlockText::Str(String::new()),
                    });
                    t.e_mut(|e| e.config_write_custom_mods(list));
                    t.update_custom_mods_controls(g);
                    t.add_undo_state(false);
                    t.build_mod_list(g);
                    t.build().build_flag.set(true);
                },
            );
            let m = me.clone();
            custom_add.base.shown.set_fn(move |_| {
                let t = m.upgrade().expect("configTab");
                !t.is_section_collapsed(&t.sections[t.custom_section])
            });
            host.add("sectionAnchor", section_anchor.clone());
            host.add("setSelect", set_select.clone());
            host.add("setLabel", set_label);
            host.add("setManage", set_manage.clone());
            host.add("search", search.clone());
            host.add("toggleConfigs", toggle_configs_button.clone());
            host.add("scrollBar", scroll_bar.clone());
            host.add("customModsAddBlock", custom_add.clone());
            Self {
                anchor: a,
                host,
                undo: UndoHandler::default(),
                me: me.clone(),
                build,
                sections,
                vars,
                var_index,
                toggle_configs: Cell::new(false),
                search,
                set_select,
                set_manage,
                toggle_configs_button,
                scroll_bar,
                section_anchor,
                custom_section,
                custom_add,
                custom_blocks: RefCell::new(Vec::new()),
                next_control: Cell::new(n),
                search_cache: RefCell::new((String::new(), None)),
                alt_down: Cell::new(false),
                pending_sel: RefCell::new(Vec::new()),
            }
        });
        // `Load` (:839-939): `SetActiveConfigSet` + `ResetUndo`; `Build.lua:645`'s `BuildModList` ran in the engine's first run
        tab.update_controls(g);
        tab.sync_placeholders(g);
        tab.reset_undo();
        tab
    }

    /// :297-346
    fn make_control(g: &Gfx, me: &Weak<Self>, sec: &ControlRef, v: &'static ConfigVar, i: usize) -> Kind {
        let var = v.var.clone().unwrap_or_default();
        let a = || anchor("TOPLEFT", sec, "TOPLEFT");
        match v.ty.as_deref() {
            Some("check") => {
                let m = me.clone();
                let var2 = var.clone();
                Kind::Check(CheckBoxControl::new(
                    g,
                    a(),
                    [234.0, 0.0, 18.0],
                    v.label.as_deref(),
                    Some(Rc::new(move |g: &mut Gfx, state: bool| {
                        let t = m.upgrade().expect("configTab");
                        t.e_mut(|e| e.config_write_input(&var2, Some(InputValue::Bool(state))));
                        t.add_undo_state(false);
                        t.build_mod_list(g);
                        t.build().build_flag.set(true);
                    })),
                    None,
                    v.default_state.as_ref().is_some_and(|s| truthy(Some(&scalar(s)))),
                ))
            }
            Some(ty @ ("count" | "integer" | "countAllowZero" | "float")) => {
                let m = me.clone();
                let var2 = var.clone();
                let filter = match ty {
                    "integer" | "countAllowZero" => "^%-%d",
                    "float" => "^%d.",
                    _ => "%D",
                };
                let e = EditControl::new(
                    g,
                    a(),
                    rect(234.0, 0.0, 90.0, 18.0),
                    EditArgs {
                        init: Some(String::new()),
                        filter: Some(filter.into()),
                        limit: Some(10),
                        change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, placeholder: bool| {
                            // :306-313; `SetPlaceholder(.., true)` runs only inside `BuildModList`, in the engine
                            assert!(!placeholder, "ConfigTab.lua:307: a placeholder write outside BuildModList");
                            let t = m.upgrade().expect("configTab");
                            t.e_mut(|e| e.config_write_input(&var2, pob_lua::tonumber(buf).map(InputValue::Num)));
                            t.add_undo_state(false);
                            t.build_mod_list(g);
                            t.build().build_flag.set(true);
                        })),
                        ..EditArgs::default()
                    },
                );
                Kind::Edit(e)
            }
            Some("list") => {
                let m = me.clone();
                let var2 = var.clone();
                let list = v
                    .list
                    .as_ref()
                    .expect("ConfigOptions: a list var without list")
                    .iter()
                    .map(|e| drop_item(&e.label, scalar(&e.val)))
                    .collect();
                let d = DropDownControl::new(
                    g,
                    a(),
                    rect(234.0, 0.0, 118.0, 16.0),
                    list,
                    Some(Rc::new(move |g: &mut Gfx, _index: usize, value: &DropItem| {
                        let t = m.upgrade().expect("configTab");
                        t.e_mut(|e| e.config_write_input(&var2, item_val(value)));
                        t.add_undo_state(false);
                        t.build_mod_list(g);
                        t.build().build_flag.set(true);
                    })),
                    None,
                    false,
                );
                if let Some(di) = v.default_index {
                    d.sel_index.set(di as usize);
                }
                Kind::Drop(d)
            }
            Some(other) => unimplemented!("Classes/ConfigTab.lua:322-343 a {other} option ({i}) has no data in this upstream"),
            None => Kind::Bare(BareControl::new(a(), rect(234.0, 0.0, 16.0, 16.0))),
        }
    }

    /// :697-708
    fn wire_border(me: &Weak<Self>, kind: &Kind, i: usize) {
        let m = me.clone();
        let f: Rc<dyn Fn() -> (f64, f64, f64)> = Rc::new(move || m.upgrade().expect("configTab").border_color(i));
        match kind {
            Kind::Check(c) => *c.border_func.borrow_mut() = Some(f),
            Kind::Edit(c) => *c.border_func.borrow_mut() = Some(f),
            Kind::Drop(c) => *c.border_func.borrow_mut() = Some(f),
            Kind::Bare(_) => {}
        }
    }

    /// :664-666, :733-798
    fn wire_tooltip(me: &Weak<Self>, kind: &Kind, i: usize, hide_if_invalid: bool) {
        if hide_if_invalid {
            // :367-376 `control.tooltipText`, read by TooltipHost
            if let Kind::Edit(c) = kind {
                let m = me.clone();
                c.tip.tooltip_text.set_fn(move |_| m.upgrade().expect("configTab").tooltip_text(i));
            }
            return;
        }
        match kind {
            Kind::Check(c) => {
                let m = me.clone();
                c.tip.set_tooltip_func(move |tt, g, _state| {
                    m.upgrade().expect("configTab").var_tooltip(tt, g, i, "", None);
                });
            }
            Kind::Edit(c) => {
                let m = me.clone();
                c.tip.set_tooltip_func(move |tt, g, ()| {
                    m.upgrade().expect("configTab").var_tooltip(tt, g, i, "", None);
                });
            }
            Kind::Drop(c) => {
                let m = me.clone();
                c.tip.set_tooltip_func(move |tt, g, (mode, _index, item)| {
                    m.upgrade().expect("configTab").var_tooltip(tt, g, i, mode, item.as_ref());
                });
            }
            Kind::Bare(_) => {}
        }
    }

    /// :835-837
    fn is_section_collapsed(&self, s: &ConfigSection) -> bool {
        s.collapsed.get() && !self.search.buf.borrow().bytes().any(|b| !b.is_ascii_whitespace())
    }

    /// :193-204 `searchMatch`.
    fn search_match(&self, v: &VarEntry) -> bool {
        let buf = self.search.buf.borrow();
        let mut cache = self.search_cache.borrow_mut();
        if cache.0 != *buf {
            cache.0.clone_from(&buf);
            let lower = buf.to_ascii_lowercase();
            let esc = pob_lua::gsub(lower.as_bytes(), b"[%-%.%+%[%]%$%^%%%?%*]", pob_lua::Repl::Str(b"%%%0"), None)
                .expect("ConfigTab.lua:194 pattern")
                .0;
            let esc = String::from_utf8_lossy(&esc).into_owned();
            cache.1 = esc.bytes().any(|b| !b.is_ascii_whitespace()).then(|| generate_or_patterns(&esc));
        }
        let Some(patterns) = &cache.1 else { return true };
        // a pattern error is the PCall's `err`: no match
        patterns
            .iter()
            .any(|p| matches!(pob_lua::match_(v.label_lower.as_bytes(), p.as_bytes(), None), Ok(Some(_))))
    }

    /// :211-229 `implyCond`.
    fn imply_cond(&self, e: &Engine, v: &VarEntry) -> bool {
        if v.imply_cond.is_empty() {
            return false;
        }
        let Some(var) = &v.data.var else { return false };
        truthy(e.config_state().input.get(var)) && v.imply_cond.iter().any(|c| e.config_gate(ConfigGate::Cond, c))
    }

    /// One entry of `shownFuncs` (:231-242).
    fn gate_passes(&self, e: &Engine, v: &VarEntry, gate: &Gate) -> bool {
        if gate.imply && self.imply_cond(e, v) {
            return true;
        }
        gate.opts.iter().any(|o| match gate.kind {
            GateKind::Option => truthy(e.config_state().input.get(o)),
            GateKind::Env(k) => e.config_gate(k, o),
        })
    }

    /// :353-364 the first `control.shown` (`innerShown`).
    fn inner_shown(&self, i: usize) -> bool {
        let v = &self.vars[i];
        if !self.search_match(v) {
            return false;
        }
        // :207-209 `isShowAllConfig`
        if self.toggle_configs.get() && !v.excluded {
            return true;
        }
        if v.gates.is_empty() {
            return true;
        }
        self.e(|e| v.gates.iter().all(|gate| self.gate_passes(e, v, gate)))
    }

    /// `not shown and cur ~= nil and cur ~= def` (:701-702, :726).
    fn invalid_state(&self, i: usize) -> bool {
        !self.inner_shown(i) && self.set_differs(i)
    }

    /// `cur ~= nil and cur ~= def`.
    fn set_differs(&self, i: usize) -> bool {
        let Some(var) = &self.vars[i].data.var else { return false };
        self.e(|e| {
            let Some(cur) = e.config_state().input.get(var) else {
                return false;
            };
            *cur != e.config_default_state(var, cur)
        })
    }

    /// :697-708
    fn border_color(&self, i: usize) -> (f64, f64, f64) {
        if self.set_differs(i) {
            if !self.inner_shown(i) {
                return (0.753, 0.502, 0.502);
            }
            return (0.451, 0.576, 0.702);
        }
        (0.5, 0.5, 0.5)
    }

    /// The final `control.shown` (:712-720, :803-808).
    fn control_shown(&self, i: usize) -> bool {
        let v = &self.vars[i];
        if self.is_section_collapsed(&self.sections[v.section]) {
            return false;
        }
        if v.hide_if_invalid {
            return self.inner_shown(i);
        }
        if !self.search_match(v) {
            return false;
        }
        self.inner_shown(i) || self.set_differs(i)
    }

    /// :367-376 `control.tooltipText`: `varData.tooltip` as a string or the two functions of ConfigOptions.lua:61-72.
    fn tooltip_text(&self, i: usize) -> Option<String> {
        let v = self.vars[i].data;
        match v.tooltip.as_ref()? {
            Json::String(s) => Some(s.clone()),
            _ => {
                let var = v.var.as_deref().unwrap_or("");
                Some(self.e(|e| match var {
                    "conditionLowLife" => format!(
                        "You will automatically be considered to be on Low ^xE05030Life ^7if you have at least {}% ^xE05030Life ^7reserved\nbut you can use this option to force it if necessary.",
                        tostring_num(100.0 - e.main_output_num("LowLifePercentage"))
                    ),
                    "conditionFullLife" => format!(
                        "You can be considered to be on Full ^xE05030Life ^7if you have at least {}% ^xE05030Life ^7left.\nYou will automatically be considered to be on Full ^xE05030Life ^7if you have Chaos Inoculation,\nbut you can use this option to force it if necessary.",
                        tostring_num(e.main_output_num("FullLifePercentage"))
                    ),
                    other => unimplemented!("Modules/ConfigOptions.lua: the tooltip function of {other}"),
                }))
            }
        }
    }

    /// `varData.tooltipFunc` (ConfigOptions.lua:11-59, :74-115).
    fn inner_tooltip_func(&self, tt: &mut Tooltip, g: &Gfx, var: &str, mode: &str, value: Option<&DropItem>) {
        let val = value.and_then(item_val);
        let val_str = val.as_ref().map(lua_tostring).unwrap_or_default();
        let apply = matches!(mode, "BODY" | "HOVER");
        match var {
            "pantheonMajorGod" | "pantheonMinorGod" => {
                tt.clear(false);
                if val_str == "None" {
                    return;
                }
                if apply {
                    let god = pob_data::pantheons::pantheons()
                        .get(&val_str)
                        .unwrap_or_else(|| panic!("ConfigOptions.lua:18: no god {val_str}"));
                    for soul in &god.souls {
                        let lines: Vec<&str> = soul.mods.iter().map(|m| m.line.as_str()).collect();
                        tt.add_line(g, 20.0, Some(&format!("^8{}", soul.name)));
                        tt.add_line(g, 14.0, Some(&format!("^6{}", lines.join("\n"))));
                        tt.add_separator(Some(10.0));
                    }
                }
            }
            "bandit" => {
                tt.clear(false);
                if apply {
                    let text = match val_str.as_str() {
                        "None" => "Grants 1 Passive Skill Point",
                        "Oak" => "+40 to Maximum ^xE05030Life",
                        "Kraityn" => "8% increased Movement Speed",
                        "Alira" => "+15% to all Elemental Resistances",
                        other => panic!("ConfigOptions.lua:42: bandit {other}"),
                    };
                    tt.add_line(g, 14.0, Some(&format!("^8{text}")));
                }
            }
            "presetBossSkills" => {
                tt.clear(false);
                if apply {
                    tt.add_line(g, 14.0, Some(BOSS_SKILLS_TIP));
                    if val_str != "None" {
                        let tip = boss_skills()["skills"][&val_str]["tooltip"]
                            .as_str()
                            .unwrap_or_else(|| panic!("ConfigOptions.lua:56: no tooltip of {val_str}"));
                        tt.add_line(g, 14.0, Some(&format!("\n^7{val_str}: {tip}")));
                    }
                }
            }
            _ if var.starts_with("MapPrefix") || var.starts_with("MapSuffix") => {
                tt.clear(false);
                if val_str == "NONE" {
                    return;
                }
                if apply {
                    map_affix_tooltip(tt, g, &val_str);
                }
            }
            other => unimplemented!("Modules/ConfigOptions.lua: the tooltipFunc of {other}"),
        }
    }

    /// :734-798
    fn var_tooltip(&self, tt: &mut Tooltip, g: &Gfx, i: usize, mode: &str, value: Option<&DropItem>) {
        let v = &self.vars[i];
        tt.clear(false);
        let var = v.data.var.as_deref().unwrap_or("");
        if v.data.tooltip_func.is_some() {
            self.inner_tooltip_func(tt, g, var, mode, value);
        } else if let Some(text) = self.tooltip_text(i)
            && !text.is_empty()
        {
            tt.add_line(g, 14.0, Some(&text));
        }
        let shown = self.inner_shown(i);
        if !shown && self.set_differs(i) {
            tt.add_line(
                g,
                14.0,
                Some(&format!(
                    "{}This config option is conditional with missing source and is invalid.",
                    color_code("NEGATIVE").expect("NEGATIVE")
                )),
            );
            return;
        }
        let ty = v.data.ty.as_deref();
        if ty != Some("check") && ty != Some("list") {
            return;
        }
        let cur = self.e(|e| e.config_state().input.get(var).cloned());
        let mapped = if ty == Some("check") {
            Some(InputValue::Bool(!truthy(cur.as_ref())))
        } else {
            value.and_then(item_val)
        };
        if mapped == cur {
            return;
        }
        tt.add_separator(Some(10.0));
        let revision = self.e(Engine::output_revision);
        let tostr = |x: &Option<InputValue>| x.as_ref().map_or_else(|| "nil".to_string(), lua_tostring);
        let key = format!("{}:{}", tostr(&mapped), tostr(&cur));
        {
            let mut cache = v.output_cache.borrow_mut();
            if cache.0 != revision {
                cache.1.clear();
                cache.0 = revision;
            }
            if !cache.1.contains_key(&key) {
                let header = if ty == Some("check") {
                    "^7Toggling this option will give you:"
                } else {
                    "^7Selecting this option will give you:"
                };
                let (m, c) = (mapped.clone(), cur.clone());
                let write = |st: &mut pob_build::ConfigState, v: Option<InputValue>| match v {
                    Some(x) => {
                        st.input.insert(var.to_owned(), x);
                    }
                    None => {
                        st.input.shift_remove(var);
                    }
                };
                let (lines, sel) = self
                    .e_mut(|e| e.config_compare(|st| write(st, m), |st| write(st, c), header))
                    .unwrap_or_else(|err| panic!("{err}"));
                *self.pending_sel.borrow_mut() = sel;
                self.apply_pending_sel();
                cache.1.insert(key.clone(), lines);
            }
            for l in &cache.1[&key] {
                tt.add_line(g, 14.0, Some(l));
            }
        }
        if tt.lines.len() == 1 {
            tt.clear(false);
        }
    }

    /// :1008-1022
    pub fn update_controls(&self, g: &mut Gfx) {
        for v in &self.vars {
            let Some(var) = &v.data.var else { continue };
            let (input, placeholder) = self.e(|e| {
                let c = e.config_state();
                (c.input.get(var).cloned(), c.placeholder.get(var).cloned())
            });
            match &v.kind {
                Kind::Edit(c) => {
                    let text = match &input {
                        None | Some(InputValue::Bool(false)) => String::new(),
                        Some(x) => lua_tostring(x),
                    };
                    c.set_text(g, &text, false);
                    if truthy(placeholder.as_ref()) {
                        c.set_placeholder(g, &lua_tostring(placeholder.as_ref().expect("truthy")), false);
                    }
                }
                Kind::Check(c) => c.state.set(truthy(input.as_ref())),
                Kind::Drop(c) => {
                    let val = if truthy(input.as_ref()) {
                        input
                    } else {
                        self.e(|e| e.config_default_state_nil(var))
                    };
                    if let Some(val) = val {
                        sel_by_value(c, &val);
                    }
                }
                Kind::Bare(_) => {}
            }
        }
        self.update_custom_mods_controls(g);
    }

    /// `:SetPlaceholder(.., true)` inside `BuildModList` and the `.enabled` writes, as the engine's tables now have them.
    fn sync_placeholders(&self, g: &mut Gfx) {
        for v in &self.vars {
            let Some(var) = &v.data.var else { continue };
            match &v.kind {
                Kind::Edit(c) => {
                    let p = self.e(|e| e.config_state().placeholder.get(var).cloned());
                    match p {
                        Some(x) => c.set_placeholder(g, &lua_tostring(&x), false),
                        // `SetPlaceholder("", true)`: `tonumber("")` deleted the key
                        None => {
                            if c.placeholder.borrow().is_some() {
                                c.set_placeholder(g, "", false);
                            }
                        }
                    }
                }
                Kind::Drop(c) => {
                    let en = self.e(|e| e.config_control_enabled(var));
                    c.base.enabled.set(en);
                }
                _ => {}
            }
        }
        // ConfigOptions.lua:789-798: `enemySizePreset`'s `SetPlaceholder(k, false)` writes the control alone
        let preset = self.e(|e| e.config_state().input.get("enemySizePreset").cloned());
        if let (Some(&i), Some(InputValue::Str(p))) = (self.var_index.get("enemyRadius"), preset)
            && let Kind::Edit(c) = &self.vars[i].kind
        {
            let k = match p.as_str() {
                "Small" => Some("2"),
                "Medium" => Some("3"),
                "Large" => Some("5"),
                "Huge" => Some("11"),
                _ => None,
            };
            if let Some(k) = k {
                c.set_placeholder(g, k, false);
            }
        }
    }

    /// A run of the engine re-applied `BuildModList` (`EstimatePlayerProgress`, R20).
    pub fn after_recalc(&self, g: &mut Gfx) {
        self.sync_placeholders(g);
    }

    fn apply_pending_sel(&self) {
        let list = std::mem::take(&mut *self.pending_sel.borrow_mut());
        for (var, index) in list {
            if let Some(&i) = self.var_index.get(var.as_str())
                && let Kind::Drop(c) = &self.vars[i].kind
            {
                c.sel_index.set(index);
            }
        }
    }

    /// `self.build.configTab.varControls[var]:SetSel(index)` (ImportTab.lua:1227).
    pub fn import_set_sel(&self, g: &mut Gfx, var: &str, index: usize) {
        if let Some(Kind::Drop(d)) = self.var_index.get(var).map(|i| &self.vars[*i].kind) {
            d.set_sel(g, index as i64, false);
        }
    }

    /// `setSelByVal(varControls[var], val)` (ImportTab.lua:1229-1235): every entry of that value, as the loop.
    pub fn import_set_sel_by_val(&self, g: &mut Gfx, var: &str, val: &str) {
        if let Some(Kind::Drop(d)) = self.var_index.get(var).map(|i| &self.vars[*i].kind) {
            let hits: Vec<usize> = d
                .list
                .borrow()
                .iter()
                .enumerate()
                .filter(|(_, v)| matches!(item_val(v), Some(InputValue::Str(s)) if s == val))
                .map(|(i, _)| i + 1)
                .collect();
            for i in hits {
                d.set_sel(g, i as i64, false);
            }
        }
    }

    /// `self:BuildModList()` (:1130-1203) from a callback.
    pub fn build_mod_list(&self, g: &mut Gfx) {
        let sel = self.e_mut(|e| e.config_build_mod_list()).unwrap_or_else(|err| panic!("{err}"));
        *self.pending_sel.borrow_mut() = sel;
        self.apply_pending_sel();
        self.sync_placeholders(g);
    }

    fn undo_g(&self, g: &mut Gfx) {
        self.undo();
        self.update_controls(g);
        self.apply_pending_sel();
        self.sync_placeholders(g);
    }

    fn redo_g(&self, g: &mut Gfx) {
        self.redo();
        self.update_controls(g);
        self.apply_pending_sel();
        self.sync_placeholders(g);
    }

    /// :1302-1334
    pub fn update_custom_mods_controls(&self, g: &mut Gfx) {
        let list = self.e(|e| e.config_state().custom_mods_list.clone());
        if list.is_empty() {
            // :1313-1315
            let legacy = self.e(|e| e.config_state().input.get("customMods").cloned());
            let mut b = pob_build::CustomModsBlockModel::default_block();
            if let Some(InputValue::Str(s)) = legacy {
                b.text = pob_build::BlockText::Str(s);
            }
            self.e_mut(|e| e.config_write_custom_mods(vec![b]));
            return self.update_custom_mods_controls(g);
        }
        for ctrl in self.custom_blocks.borrow().iter() {
            ctrl.base.shown.set(false);
        }
        self.custom_blocks.borrow_mut().clear();
        let sec = &self.sections[self.custom_section];
        let custom_add: ControlRef = self.custom_add.clone();
        *sec.var_control_list.borrow_mut() = vec![custom_add];
        let sec_ref: ControlRef = sec.control.clone();
        for (index, block) in list.iter().enumerate() {
            let c = CustomModBlock::new(g, anchor("TOPLEFT", &sec_ref, "TOPLEFT"), self.me.clone(), index, block);
            let m = self.me.clone();
            c.base.shown.set_fn(move |_| {
                let t = m.upgrade().expect("configTab");
                !t.is_section_collapsed(&t.sections[t.custom_section])
            });
            self.custom_blocks.borrow_mut().push(c.clone());
            // `t_insert(self.controls, blockControl)`: the old ones stay there, hidden
            let n = self.next_control.get() + 1;
            self.next_control.set(n);
            self.host.add(&format!("#{n}"), c.clone());
            sec.var_control_list.borrow_mut().push(c);
        }
    }

    /// :1337-1362, `init` nil.
    pub fn set_active_config_set(&self, g: &mut Gfx, id: f64) {
        self.e_mut(|e| e.config_activate(id));
        self.update_controls(g);
        self.build_mod_list(g);
        self.build().build_flag.set(true);
        self.build().sync_loadouts_pub(g);
    }

    /// :1024-1116
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let a = &self.anchor.base;
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        self.alt_down.set(sg::is_key_down(f, "ALT"));
        let downs: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in downs {
            if sg::is_key_down(f, "CTRL") {
                match key.as_str() {
                    "z" => {
                        self.undo_g(f);
                        self.build().build_flag.set(true);
                    }
                    "y" => {
                        self.redo_g(f);
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
        let max_col = ((view_port.width - 10.0) / 370.0).floor() as i64;
        let mut max_col_y: f64 = 0.0;
        let mut col_y: HashMap<i64, f64> = HashMap::new();
        col_y.insert(1, 0.0);
        for section in &self.sections {
            let mut y = 14.0;
            section.control.base.shown.set(true);
            let collapsed = section.collapsed.get();
            section.collapsed.set(false);
            let mut do_show = false;
            let list = section.var_control_list.borrow().clone();
            for c in &list {
                if c.is_shown(f) {
                    do_show = true;
                    let (_, height) = c.get_size(f);
                    let height = height.max(16.0);
                    c.base().y.set(y + 2.0);
                    y += height + 4.0;
                }
            }
            section.collapsed.set(collapsed);
            section.control.base.shown.set(do_show);
            if do_show {
                let (_, height) = section.control.get_size(f);
                let sec_col = section.col as i64;
                let col = if section.col_fits(col_y.get(&sec_col).copied(), height, view_port) {
                    sec_col
                } else {
                    let mut col = 1;
                    for c in 2..=max_col {
                        let cy = *col_y.entry(c).or_insert(0.0);
                        if cy < col_y[&col] {
                            col = c;
                        }
                    }
                    col
                };
                let cy = *col_y.entry(col).or_insert(0.0);
                section.control.base.x.set(10.0 + (col - 1) as f64 * 370.0);
                section.control.base.y.set(cy + 18.0);
                let ny = cy + height + 18.0;
                col_y.insert(col, ny);
                max_col_y = max_col_y.max(ny);
            }
        }
        // :1099-1107
        let (order, active) = self.e(|e| (e.config_set_order(), e.config_active_set()));
        let mut new_list = Vec::with_capacity(order.len());
        for (index, id) in order.iter().enumerate() {
            new_list.push(DropItem::str(
                &self.e(|e| e.config_set_title(*id)).unwrap_or_else(|| "Default".to_string()),
            ));
            if *id == active {
                self.set_select.sel_index.set(index + 1);
            }
        }
        self.set_select.set_list(f, Some(new_list));
        self.scroll_bar.base.height.set(view_port.height);
        self.scroll_bar.set_content_dimension(f, max_col_y + 58.0, view_port.height);
        self.section_anchor.base.y.set(20.0 - self.scroll_bar.offset.get());
        main().draw_background(f, view_port);
        self.host.draw_controls(f, view_port, None);
    }

    /// The state the gate compares (the same fields as `config-runner.lua`).
    pub fn debug_dump(&self, g: &Gfx) -> Json {
        let n = |x: f64| tostring_num(x);
        let rect_of = |c: &dyn Control| {
            let (x, y) = c.get_pos(g);
            let (w, h) = c.get_size(g);
            format!("{},{},{},{}", n(x), n(y), n(w), n(h))
        };
        let (order, active, titles, input, placeholder, enemy_level, blocks) = self.e(|e| {
            let c = e.config_state();
            let order = e.config_set_order();
            let titles: Vec<String> = order
                .iter()
                .map(|id| e.config_set_title(*id).unwrap_or_else(|| "nil".into()))
                .collect();
            let conv = |m: &IndexMap<String, InputValue>| -> serde_json::Map<String, Json> {
                m.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            match v {
                                InputValue::Num(x) => json!(x),
                                InputValue::Str(s) => json!(s),
                                InputValue::Bool(b) => json!(b),
                            },
                        )
                    })
                    .collect()
            };
            let blocks: Vec<Json> = c
                .custom_mods_list
                .iter()
                .map(|b| json!({ "title": b.title, "enabled": b.enabled, "text": b.text.as_str().unwrap_or("") }))
                .collect();
            (
                order,
                e.config_active_set(),
                titles,
                conv(&c.input),
                conv(&c.placeholder),
                c.enemy_level,
                blocks,
            )
        });
        let border = |f: &RefCell<Option<Rc<dyn Fn() -> (f64, f64, f64)>>>| {
            let (r, gr, b) = f.borrow().as_ref().expect("borderFunc")();
            format!("{},{},{}", n(r), n(gr), n(b))
        };
        let mut sections = Vec::new();
        for s in &self.sections {
            let mut rows = Vec::new();
            let list = s.var_control_list.borrow().clone();
            for c in &list {
                if !c.is_shown(g) {
                    continue;
                }
                let var = self.vars.iter().find(|v| same(&*v.kind.control(), &**c));
                let row = match var {
                    Some(v) => {
                        let name = v.data.var.as_deref().unwrap_or("-");
                        match &v.kind {
                            Kind::Check(cb) => format!(
                                "check|{name}|{}|{}|{}|{}",
                                rect_of(&**cb),
                                cb.state.get(),
                                cb.base.enabled.get(g),
                                border(&cb.border_func)
                            ),
                            Kind::Edit(ed) => format!(
                                "edit|{name}|{}|{}|{}|{}|{}",
                                rect_of(&**ed),
                                ed.buf.borrow(),
                                ed.placeholder.borrow().clone().unwrap_or_default(),
                                ed.base.enabled.get(g),
                                border(&ed.border_func)
                            ),
                            Kind::Drop(dd) => format!(
                                "list|{name}|{}|{}|{}|{}|{}",
                                rect_of(&**dd),
                                dd.sel_index.get(),
                                dd.base.enabled.get(g),
                                dd.dropped.get(),
                                border(&dd.border_func)
                            ),
                            Kind::Bare(b) => format!("bare|{name}|{}", rect_of(&**b)),
                        }
                    }
                    None => {
                        if same(&**c, &*self.custom_add) {
                            format!("add|{}", rect_of(&**c))
                        } else {
                            let bl = self.custom_blocks.borrow();
                            let b = bl.iter().find(|b| same(&***b, &**c)).expect("a custom block");
                            format!(
                                "block|{}|{}|{}|{}",
                                rect_of(&**c),
                                b.title_edit.buf.borrow(),
                                b.enable_check.state.get(),
                                b.text_edit.edit.buf.borrow()
                            )
                        }
                    }
                };
                rows.push(json!(row));
            }
            sections.push(json!({
                "label": s.control.label.get(g),
                "shown": s.control.is_shown(g),
                "collapsed": s.collapsed.get(),
                "rect": if s.control.is_shown(g) { rect_of(&*s.control) } else { "hidden".to_string() },
                "toggle": s.toggle.label.get(g),
                "rows": rows,
            }));
        }
        let mut labels = Vec::new();
        for v in &self.vars {
            if let Some(l) = &v.label_control
                && l.is_shown(g)
            {
                let (x, y) = l.get_pos(g);
                labels.push(json!(format!("{},{}|{}", n(x), n(y), l.label.get(g))));
            }
        }
        let mut tips = serde_json::Map::new();
        for v in &self.vars {
            let Some(var) = &v.data.var else { continue };
            let lines: Vec<String> = match &v.kind {
                Kind::Check(c) => tip_lines(&c.tip.tooltip.borrow()),
                Kind::Edit(c) => tip_lines(&c.tip.tooltip.borrow()),
                Kind::Drop(c) => tip_lines(&c.tip.tooltip.borrow()),
                Kind::Bare(_) => Vec::new(),
            };
            if !lines.is_empty() {
                tips.insert(var.clone(), json!(lines));
            }
        }
        let sel = match self.host.sel() {
            None => "nil".to_string(),
            Some(c) => {
                if same(&*c, &*self.search) {
                    "search".into()
                } else if same(&*c, &*self.scroll_bar) {
                    "scrollBar".into()
                } else if same(&*c, &*self.set_select) {
                    "setSelect".into()
                } else if let Some(v) = self.vars.iter().find(|v| same(&*v.kind.control(), &*c)) {
                    format!("var:{}", v.data.var.as_deref().unwrap_or("-"))
                } else {
                    "other".into()
                }
            }
        };
        let set_list: Vec<String> = self
            .set_select
            .list
            .borrow()
            .iter()
            .map(|i| i.label.clone().unwrap_or_default())
            .collect();
        json!({
            "active": active,
            "order": order,
            "titles": titles,
            "search": self.search.buf.borrow().clone(),
            "scroll": self.scroll_bar.offset.get(),
            "toggleConfigs": self.toggle_configs.get(),
            "toggleLabel": self.toggle_configs_button.label.get(g),
            "sel": sel,
            "modFlag": self.undo.mod_flag.get(),
            "undo": self.undo.undo.borrow().len(),
            "redo": self.undo.redo.borrow().len(),
            "enemyLevel": enemy_level,
            "input": input,
            "placeholder": placeholder,
            "blocks": blocks,
            "setSelect": {
                "sel": self.set_select.sel_index.get(),
                "enabled": self.set_select.base.enabled.get(g),
                "dropped": self.set_select.dropped.get(),
                "list": set_list,
            },
            "sections": sections,
            "labels": labels,
            "tips": tips,
        })
    }

    /// `scrollVar` of the gate.
    pub fn debug_scroll_to_var(&self, g: &Gfx, var: &str) {
        let i = *self.var_index.get(var).unwrap_or_else(|| panic!("var {var}"));
        let (_, y) = self.vars[i].kind.control().get_pos(g);
        self.scroll_bar.set_offset(self.scroll_bar.offset.get() + y - 300.0);
    }

    /// `scrollBlock` of the gate.
    pub fn debug_scroll_to_block(&self, g: &Gfx, index: usize) {
        let (_, y) = self.custom_blocks.borrow()[index - 1].get_pos(g);
        self.scroll_bar.set_offset(self.scroll_bar.offset.get() + y - 300.0);
    }

    /// `cursorVar` of the gate: the middle of the control of a var.
    pub fn debug_var_pos(&self, g: &Gfx, var: &str) -> (f64, f64) {
        let i = *self.var_index.get(var).unwrap_or_else(|| panic!("var {var}"));
        let c = self.vars[i].kind.control();
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        ((x + w / 2.0).floor(), (y + h / 2.0).floor())
    }

    /// `cursorDrop` of the gate: the row `index` (1-based) of the open dropdown of a var.
    pub fn debug_drop_row_pos(&self, g: &Gfx, var: &str, index: usize) -> (f64, f64) {
        let i = *self.var_index.get(var).unwrap_or_else(|| panic!("var {var}"));
        let Kind::Drop(d) = &self.vars[i].kind else {
            panic!("{var} is no dropdown")
        };
        let (x, y) = d.get_pos(g);
        let (w, h) = d.get_size(g);
        ((x + w / 2.0).floor(), y + h + 2.0 + (index as f64 - 1.0) * (h - 4.0) + 4.0)
    }

    /// `cursorBlock` of the gate: a control of a custom block (`delete`, `title`, `add`, `check`, `text`).
    pub fn debug_block_pos(&self, g: &Gfx, index: usize, part: &str) -> (f64, f64) {
        let b = self.custom_blocks.borrow()[index - 1].clone();
        let c: ControlRef = match part {
            "delete" => b.delete_btn.clone(),
            "title" => b.title_edit.clone(),
            "add" => b.add_mod_btn.clone(),
            "check" => b.enable_check.clone(),
            "text" => b.text_edit.clone(),
            other => panic!("block part {other}"),
        };
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        ((x + w / 2.0).floor(), (y + h / 2.0).floor())
    }

    /// `cursorSection` of the gate: the collapse toggle of a section.
    pub fn debug_section_toggle_pos(&self, g: &Gfx, label: &str) -> (f64, f64) {
        let s = self
            .sections
            .iter()
            .find(|s| s.control.label.get(g) == label)
            .unwrap_or_else(|| panic!("section {label}"));
        let (x, y) = s.toggle.get_pos(g);
        (x + 8.0, y + 8.0)
    }
}

fn tip_lines(t: &Tooltip) -> Vec<String> {
    t.lines
        .iter()
        .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
        .collect()
}

impl ConfigSection {
    /// :1080 the fixed column of the section: it fits this column, and the column fits the view.
    fn col_fits(&self, col_y: Option<f64>, height: f64, view_port: &Rect) -> bool {
        self.col != 0.0 && col_y.unwrap_or(0.0) + height + 28.0 <= view_port.height && 10.0 + self.col * 370.0 <= view_port.width
    }
}

fn boss_skills() -> &'static Json {
    static M: std::sync::OnceLock<Json> = std::sync::OnceLock::new();
    M.get_or_init(|| pob_data::read_json("boss-skills.json").unwrap_or_else(|e| panic!("{e}")))
}

fn map_mods() -> &'static Json {
    static M: std::sync::OnceLock<Json> = std::sync::OnceLock::new();
    M.get_or_init(|| pob_data::read_json("map-mods.json").unwrap_or_else(|e| panic!("{e}")))
}

/// `mapAffixTooltip` (ConfigOptions.lua:81-113), from `tooltip:AddLine(14, '^7'..value.val)`.
fn map_affix_tooltip(tt: &mut Tooltip, g: &Gfx, val: &str) {
    use pob_lua::format as lua_format;
    tt.add_line(g, 14.0, Some(&format!("^7{val}")));
    let affix = &map_mods()["AffixData"][val];
    let lines: Vec<&str> = affix["tooltipLines"]
        .as_array()
        .unwrap_or_else(|| panic!("ConfigOptions.lua:83: attempt to get length of field 'tooltipLines' ({val})"))
        .iter()
        .filter_map(Json::as_str)
        .collect();
    if lines.is_empty() {
        return;
    }
    let values = &affix["values"];
    let num = |j: &Json| pob_data::wire::json_num(j);
    let fmt1 = |line: &str, v: f64| lua_format(line, &[pob_lua::format::Arg::Num(v)]).unwrap_or_else(|e| panic!("{e}"));
    let fmt2 = |line: &str, a: f64, b: f64| {
        lua_format(line, &[pob_lua::format::Arg::Num(a), pob_lua::format::Arg::Num(b)]).unwrap_or_else(|e| panic!("{e}"))
    };
    match affix["type"].as_str() {
        Some("check") => {
            for line in &lines {
                tt.add_line(g, 14.0, Some(&format!("^7{line}")));
            }
        }
        Some("list") => {
            for (i, tier) in ["Low", "Med", "High"].iter().enumerate() {
                tt.add_line(g, 16.0, Some(&format!("^7{tier}: ")));
                for (j, line) in lines.iter().enumerate() {
                    let mv = if lines.len() > 1 { &values[i][j] } else { &values[i] };
                    match num(mv) {
                        None => tt.add_line(g, 14.0, Some(&format!("   ^7{line}"))),
                        Some(v) if v != 0.0 => tt.add_line(g, 14.0, Some(&format!("   ^7{}", fmt1(line, v)))),
                        Some(_) => {}
                    }
                }
            }
        }
        Some("count") => {
            for (i, tier) in ["Low", "Med", "High"].iter().enumerate() {
                tt.add_line(g, 16.0, Some(&format!("^7{tier}: ")));
                for (j, line) in lines.iter().enumerate() {
                    let v = if lines.len() > 1 { &values[i][j] } else { &values[i] };
                    match (num(&v[0]), num(&v[1])) {
                        (_, None) => tt.add_line(g, 14.0, Some(&format!("   ^7{line}"))),
                        (Some(a), Some(b)) if b != 0.0 => tt.add_line(g, 14.0, Some(&format!("   ^7{}", fmt2(line, a, b)))),
                        (None, Some(b)) if b != 0.0 => panic!("ConfigOptions.lua:108: bad argument to format ({val})"),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
}

// --- CustomModBlockControl (:16-127) ---------------------------------------------------------------------------------------------------

pub struct CustomModBlock {
    pub base: ControlBase,
    pub host: ControlHost,
    pub delete_btn: Rc<ButtonControl>,
    pub title_edit: Rc<EditControl>,
    pub add_mod_btn: Rc<ButtonControl>,
    pub enable_check: Rc<CheckBoxControl>,
    pub text_edit: Rc<ResizableEditControl>,
}

/// `modLib.parseMod(line)`: `mods and not extra` (ConfigTab.lua:90).
fn line_is_supported(line: &str) -> bool {
    thread_local! {
        static CACHE: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    }
    CACHE.with_borrow_mut(|c| {
        if let Some(a) = c.get(line) {
            return *a;
        }
        let p = pob_parser::parse_mod_line(line, false, pob_parser::ParseMode::Full);
        let a = p.mod_list.is_some() && p.extra.is_none();
        c.insert(line.to_string(), a);
        a
    })
}

fn trim_lua(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
}

impl CustomModBlock {
    fn new(g: &Gfx, a: Option<Anchor>, tab: Weak<ConfigTab>, index: usize, block: &pob_build::CustomModsBlockModel) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let me_dyn: WeakControl = me.clone();
            let host = ControlHost::new();
            let t = tab.clone();
            let delete_btn = ButtonControl::new(
                anchor_weak("TOPLEFT", me_dyn.clone(), "TOPLEFT"),
                rect(0.0, 0.0, 20.0, 18.0),
                "^1X".to_string(),
                move |g| {
                    let t = t.upgrade().expect("configTab");
                    let mut list = t.e(|e| e.config_state().custom_mods_list.clone());
                    list.remove(index);
                    if list.is_empty() {
                        list.push(pob_build::CustomModsBlockModel::default_block());
                    }
                    t.e_mut(|e| e.config_write_custom_mods(list));
                    t.update_custom_mods_controls(g);
                    t.add_undo_state(false);
                    t.build_mod_list(g);
                    t.build().build_flag.set(true);
                },
            );
            let del_ref: ControlRef = delete_btn.clone();
            let t = tab.clone();
            let title_edit = EditControl::new(
                g,
                anchor("LEFT", &del_ref, "RIGHT"),
                rect(6.0, 0.0, 222.0, 18.0),
                EditArgs {
                    init: Some(block.title.clone()),
                    change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _p: bool| {
                        let t = t.upgrade().expect("configTab");
                        t.edit_block(index, |b| b.title = buf.to_string());
                        t.add_undo_state(false);
                        t.build_mod_list(g);
                        t.build().build_flag.set(true);
                    })),
                    ..EditArgs::default()
                },
            );
            let title_ref: ControlRef = title_edit.clone();
            let t = tab.clone();
            let add_mod_btn = ButtonControl::new(
                anchor("LEFT", &title_ref, "RIGHT"),
                rect(6.0, 0.0, 58.0, 18.0),
                "^7Add Mod".to_string(),
                move |g| super::config_mod_browser::open_add_mod_popup(g, t.clone(), index),
            );
            let t = tab.clone();
            let enable_check = CheckBoxControl::new(
                g,
                anchor_weak("TOPRIGHT", me_dyn.clone(), "TOPRIGHT"),
                [0.0, 0.0, 18.0],
                Some(""),
                Some(Rc::new(move |g: &mut Gfx, state: bool| {
                    let t = t.upgrade().expect("configTab");
                    t.edit_block(index, |b| b.enabled = state);
                    t.add_undo_state(false);
                    t.build_mod_list(g);
                    t.build().build_flag.set(true);
                })),
                None,
                block.enabled,
            );
            let t = tab.clone();
            enable_check.tip.set_tooltip_func(move |tt, g, _state| {
                let t = t.upgrade().expect("configTab");
                let revision = t.e(Engine::output_revision);
                if tt.check_for_update((revision, index)) {
                    let cur = t.e(|e| e.config_state().custom_mods_list.get(index).is_some_and(|b| b.enabled));
                    let header = if cur {
                        "^7Disabling this group will give you:"
                    } else {
                        "^7Enabling this group will give you:"
                    };
                    let (lines, _) = t
                        .e_mut(|e| {
                            e.config_compare(
                                |st| {
                                    if let Some(b) = st.custom_mods_list.get_mut(index) {
                                        b.enabled = !cur;
                                    }
                                },
                                |st| {
                                    if let Some(b) = st.custom_mods_list.get_mut(index) {
                                        b.enabled = cur;
                                    }
                                },
                                header,
                            )
                        })
                        .unwrap_or_else(|err| panic!("{err}"));
                    for l in lines {
                        tt.add_line(g, 14.0, Some(&l));
                    }
                }
            });
            let t = tab.clone();
            let text_edit = ResizableEditControl::new(
                g,
                anchor_weak("TOPLEFT", me_dyn, "TOPLEFT"),
                [
                    Some(0.0),
                    Some(22.0),
                    Some(344.0),
                    Some(80.0),
                    Some(344.0),
                    Some(40.0),
                    Some(344.0),
                    Some(600.0),
                ],
                EditArgs {
                    init: Some(block.text.as_str().unwrap_or("").to_string()),
                    filter: Some("^%C\t\n".into()),
                    change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _p: bool| {
                        let t = t.upgrade().expect("configTab");
                        t.edit_block(index, |b| b.text = pob_build::BlockText::Str(buf.to_string()));
                        t.add_undo_state(false);
                        t.build_mod_list(g);
                        t.build().build_flag.set(true);
                    })),
                    line_height: Some(16.0),
                    ..EditArgs::default()
                },
            );
            let t = tab.clone();
            *text_edit.edit.inactive_text.borrow_mut() = Some(InactiveText::Fn(Rc::new(move |val: &str| {
                let alt = t.upgrade().expect("configTab").alt_down.get();
                let magic = color_code("MAGIC").expect("MAGIC");
                let unsupported = color_code("UNSUPPORTED").expect("UNSUPPORTED");
                let bytes = val.as_bytes();
                let mut out = String::new();
                for caps in pob_lua::gmatch(bytes, b"([^\n]*)\n?") {
                    let caps = caps.expect("ConfigTab.lua:88 pattern");
                    let Some(pob_lua::Capture::Str { start, end }) = caps.get(0) else {
                        continue;
                    };
                    let line = &val[start..end];
                    let stripped = strip(line);
                    let stripped = trim_lua(&stripped);
                    out.push_str(if line_is_supported(stripped) { &magic } else { &unsupported });
                    out.push_str(if alt { stripped } else { line });
                    out.push('\n');
                }
                out
            })));
            host.add("deleteBtn", delete_btn.clone());
            host.add("titleEdit", title_edit.clone());
            host.add("addModBtn", add_mod_btn.clone());
            host.add("enableCheck", enable_check.clone());
            host.add("textEdit", text_edit.clone());
            Self {
                base: ControlBase::new(a, rect(8.0, 0.0, 344.0, 120.0)),
                host,
                delete_btn,
                title_edit,
                add_mod_btn,
                enable_check,
                text_edit,
            }
        })
    }
}

impl Control for CustomModBlock {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    /// :98-102
    fn get_size(&self, g: &Gfx) -> (f64, f64) {
        let text_height = self.text_edit.get_size(g).1;
        (344.0, 22.0 + text_height + 4.0)
    }
    /// :104-109
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        if !self.is_shown(g) {
            return Some(false);
        }
        Some(self.is_mouse_in_bounds(g) || self.host.get_mouse_over_control(g).is_some())
    }
    fn has_on_key_down(&self) -> bool {
        true
    }
    /// :111-119
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let over = self.host.get_mouse_over_control(g)?;
        if over.has_on_key_down() {
            return over.on_key_down(g, key, double_click);
        }
        None
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// :121-127
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, _no_tooltip: bool) {
        if !self.is_shown(f) {
            return;
        }
        self.host.draw_controls(f, view_port, None);
    }
}

impl ConfigTab {
    /// `blockData.x = ...` of the block at this index.
    pub(super) fn edit_block(&self, index: usize, edit: impl FnOnce(&mut pob_build::CustomModsBlockModel)) {
        let mut list = self.e(|e| e.config_state().custom_mods_list.clone());
        if let Some(b) = list.get_mut(index) {
            edit(b);
        }
        self.e_mut(|e| e.config_write_custom_mods(list));
    }

    pub(super) fn block_text(&self, index: usize) -> String {
        self.e(|e| {
            e.config_state()
                .custom_mods_list
                .get(index)
                .and_then(|b| b.text.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
    }
}

// --- ConfigSetListControl ------------------------------------------------------------------------------------------------------------------

struct SetListHooks {
    tab: Weak<ConfigTab>,
    list: Rc<RefCell<Weak<SetList>>>,
}

type SetList = ListControl<f64>;

impl ListHooks<f64> for SetListHooks {
    /// ConfigSetListControl.lua:75-80
    fn get_row_value(&self, _l: &SetList, _g: &Gfx, column: usize, _index: usize, id: &f64) -> String {
        if column != 1 {
            return String::new();
        }
        let t = self.tab.upgrade().expect("configTab");
        let (title, active) = t.e(|e| (e.config_set_title(*id), e.config_active_set()));
        format!(
            "{}{}",
            title.unwrap_or_else(|| "Default".into()),
            if *id == active { "  ^9(Current)" } else { "" }
        )
    }

    /// :82-84
    fn on_order_change(&self, l: &SetList, _old: usize, _new: usize) {
        let t = self.tab.upgrade().expect("configTab");
        let order = l.list.borrow().clone();
        t.e_mut(|e| e.config_write_order(&order));
        t.undo.mod_flag.set(true);
    }

    /// :86-91
    fn on_sel_click(&self, _l: &SetList, g: &mut Gfx, _index: usize, id: &f64, double_click: bool) -> Option<bool> {
        let t = self.tab.upgrade().expect("configTab");
        if double_click && *id != t.e(Engine::config_active_set) {
            t.set_active_config_set(g, *id);
            t.add_undo_state(false);
        }
        None
    }

    /// :93-108
    fn on_sel_delete(&self, _l: &SetList, g: &mut Gfx, index: usize, id: &f64) {
        delete_set(g, self.tab.clone(), self.list.borrow().clone(), index, *id);
    }

    /// :110-114
    fn on_sel_key_down(&self, _l: &SetList, g: &mut Gfx, _index: usize, id: &f64, key: &str) {
        if key == "F2" {
            let t = self.tab.upgrade().expect("configTab");
            let title = t.e(|e| e.config_set_title(*id));
            rename_set(g, self.tab.clone(), self.list.borrow().clone(), *id, title, None);
        }
    }
}

/// :93-108
fn delete_set(g: &mut Gfx, tab: Weak<ConfigTab>, list: Weak<SetList>, index: usize, id: f64) {
    let l = list.upgrade().expect("list");
    if l.list.borrow().len() <= 1 {
        return;
    }
    let t = tab.upgrade().expect("configTab");
    let title = t.e(|e| e.config_set_title(id)).unwrap_or_else(|| "Default".into());
    main().open_confirm_popup(
        g,
        "Delete Config Set",
        &format!("Are you sure you want to delete '{title}'?"),
        "Delete",
        Rc::new(move |g: &mut Gfx| {
            let l = list.upgrade().expect("list");
            let t = tab.upgrade().expect("configTab");
            l.list.borrow_mut().remove(index - 1);
            let order = l.list.borrow().clone();
            t.e_mut(|e| {
                e.config_write_order(&order);
                e.config_remove_set(id);
            });
            l.sel_index.set(None);
            *l.sel_value.borrow_mut() = None;
            if id == t.e(Engine::config_active_set) {
                let next = order[index.saturating_sub(1).max(1) - 1];
                t.set_active_config_set(g, next);
            }
            t.add_undo_state(false);
            t.build().sync_loadouts_pub(g);
        }),
        None,
    );
}

/// :47-73 `RenameSet`: `new` = the set not yet in the list (`addOnName`).
fn rename_set(
    g: &mut Gfx,
    tab: Weak<ConfigTab>,
    list: Weak<SetList>,
    id: f64,
    title: Option<String>,
    new: Option<pob_build::ConfigSetModel>,
) {
    let popup_title = if title.is_some() { "Rename" } else { "Set Name" };
    let label = LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter name for this config set:".to_string());
    let save_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let sc = save_cell.clone();
    let edit = EditControl::new(
        g,
        None,
        rect(0.0, 40.0, 350.0, 20.0),
        EditArgs {
            init: Some(title.clone().unwrap_or_default()),
            limit: Some(100),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _p: bool| {
                let has = buf.bytes().any(|b| !b.is_ascii_whitespace());
                sc.borrow().as_ref().expect("save").base.enabled.set(has);
            })),
            ..EditArgs::default()
        },
    );
    let e2 = edit.clone();
    let tab2 = tab.clone();
    let new_cell = RefCell::new(new);
    let list2 = list.clone();
    let save = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
        let t = tab2.upgrade().expect("configTab");
        let text = e2.buf.borrow().clone();
        let added = new_cell.borrow_mut().take();
        let was_new = added.is_some();
        t.e_mut(|e| {
            if let Some(s) = added {
                e.config_insert_set(s);
            }
            e.config_rename_set(id, &text);
        });
        t.undo.mod_flag.set(true);
        if was_new {
            let l = list2.upgrade().expect("list");
            l.list.borrow_mut().push(id);
            l.sel_index.set(Some(l.list.borrow().len()));
            *l.sel_value.borrow_mut() = Some(id);
        }
        t.add_undo_state(false);
        t.build().sync_loadouts_pub(g);
        main().close_popup();
    });
    save.base.enabled.set(false);
    *save_cell.borrow_mut() = Some(save.clone());
    let cancel = ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup());
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert("label".into(), label);
    controls.insert("edit".into(), edit);
    controls.insert("save".into(), save);
    controls.insert("cancel".into(), cancel);
    main().open_popup(
        g,
        370.0,
        100.0,
        popup_title,
        controls,
        Some("save"),
        Some("edit"),
        Some("cancel"),
        None,
    );
}

fn popup_control(key: &str) -> ControlRef {
    let p = main().popups.borrow().first().cloned().expect("popup");
    p.host.get(key).unwrap_or_else(|| panic!("popup control {key}"))
}

/// `popup` of the gate's dump (`config-runner.lua`).
pub fn debug_popup(g: &Gfx) -> Json {
    let Some(p) = main().popups.borrow().first().cloned() else {
        return Json::Null;
    };
    let mut o = serde_json::Map::new();
    o.insert("title".into(), json!(p.title.get(g)));
    o.insert("width".into(), json!(p.base.width.get(g)));
    o.insert("height".into(), json!(p.base.height.get(g)));
    if let Some(c) = p.host.get("1") {
        let any: Rc<dyn Any> = c;
        if let Ok(l) = any.downcast::<SetList>() {
            let rows: Vec<String> = l
                .list
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, v)| l.hooks.get_row_value(&l, g, 1, i + 1, v))
                .collect();
            o.insert("rows".into(), json!(rows));
            o.insert("selIndex".into(), json!(l.sel_index.get().unwrap_or(0)));
        }
    }
    if let Some(c) = p.host.get("listControl") {
        super::config_mod_browser::debug_browser(g, &p, c, &mut o);
    }
    if let Some(c) = p.host.get("edit") {
        let any: Rc<dyn Any> = c;
        let e = any.downcast::<EditControl>().expect("edit");
        o.insert("edit".into(), json!(e.buf.borrow().clone()));
        o.insert("saveEnabled".into(), json!(p.host.get("save").is_some_and(|s| s.is_enabled(g))));
    }
    Json::Object(o)
}

/// The state of the real window (config-rust-input.ps1): the same fields as `RealState` of config-runner.lua. `null` until the tab exists.
pub fn debug_input_state(g: &Gfx) -> Json {
    let b = main().build_mode.clone();
    if main().mode.borrow().as_deref() != Some("BUILD") {
        return Json::Null;
    }
    let Some(t) = b.config_tab.borrow().clone() else {
        return Json::Null;
    };
    let pos = |c: &dyn Control| {
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        json!({ "x": (x + w / 2.0).floor(), "y": (y + h / 2.0).floor() })
    };
    let check = t.vars[t.var_index["buffArcaneSurge"]].kind.clone();
    let edit = t.vars[t.var_index["multiplierMapModEffect"]].kind.clone();
    let (Kind::Check(check), Kind::Edit(edit)) = (check, edit) else {
        panic!("the kinds of the two options");
    };
    let (input_check, input_edit) = t.e(|e| {
        let i = &e.config_state().input;
        (
            truthy(i.get("buffArcaneSurge")),
            match i.get("multiplierMapModEffect") {
                Some(InputValue::Num(n)) => json!(n),
                _ => Json::Null,
            },
        )
    });
    let stat: Vec<String> = b
        .c()
        .stat_box
        .list
        .borrow()
        .iter()
        .map(|l| {
            let col = |i: usize| l.cols.get(i).cloned().flatten().unwrap_or("nil".into());
            format!("{}|{}", col(0), col(1))
        })
        .collect();
    json!({
        "viewMode": *b.view_mode.borrow(),
        "modeBtn": b.host.get("modeConfig").map(|c| pos(&*c)),
        "checkPos": pos(&*check),
        "editPos": pos(&*edit),
        "check": check.state.get(),
        "buf": edit.buf.borrow().clone(),
        "inputCheck": input_check,
        "inputEdit": input_edit,
        "scroll": t.scroll_bar.offset.get(),
        "statBox": stat,
    })
}

/// `cursorList` of the gate.
pub fn debug_list_button_pos(g: &Gfx, list: &str, button: &str) -> (f64, f64) {
    let any: Rc<dyn Any> = popup_control(list);
    let l = any.downcast::<SetList>().expect("the sets list");
    let c = l.host.get(button).unwrap_or_else(|| panic!("button {button}"));
    let (x, y) = c.get_pos(g);
    let (w, h) = c.get_size(g);
    ((x + w / 2.0).floor(), (y + h / 2.0).floor())
}

/// `cursorRow` of the gate.
pub fn debug_list_row_pos(g: &Gfx, list: &str, row: usize) -> (f64, f64) {
    let any: Rc<dyn Any> = popup_control(list);
    match any.downcast::<SetList>() {
        Ok(l) => {
            let r = l.get_row_region(g);
            let (x, y) = l.get_pos(g);
            (x + r.x + 20.0, y + r.y + (row as f64 - 1.0) * l.row_height + 8.0)
        }
        Err(any) => {
            let l = any
                .downcast::<ListControl<crate::controls::list::ByRef<super::config_mod_browser::ModEntry>>>()
                .expect("a list");
            let r = l.get_row_region(g);
            let (x, y) = l.get_pos(g);
            (x + r.x + 20.0, y + r.y + (row as f64 - 1.0) * l.row_height + 8.0)
        }
    }
}

/// :1270-1277 + ConfigSetListControl.lua:13-45
fn open_config_set_manage_popup(g: &mut Gfx, tab: Weak<ConfigTab>) {
    let t = tab.upgrade().expect("configTab");
    let hook_list: Rc<RefCell<Weak<SetList>>> = Rc::new(RefCell::new(Weak::new()));
    let list = ListControl::new(
        None,
        rect(0.0, 50.0, 350.0, 200.0),
        16.0,
        Some("VERTICAL"),
        true,
        t.e(Engine::config_set_order),
        false,
        Box::new(SetListHooks {
            tab: tab.clone(),
            list: hook_list.clone(),
        }),
    );
    *hook_list.borrow_mut() = Rc::downgrade(&list);
    let lr: ControlRef = list.clone();
    let lw = Rc::downgrade(&list);
    let (w, tb) = (lw.clone(), tab.clone());
    let copy = ButtonControl::new(
        anchor("BOTTOMLEFT", &lr, "TOP"),
        rect(2.0, -4.0, 60.0, 18.0),
        "Copy".to_string(),
        move |g| {
            let t = tb.upgrade().expect("configTab");
            let id = w.upgrade().expect("list").sel_value.borrow().expect("selValue");
            let set = t.e(|e| e.config_copy_set(id));
            rename_set(g, tb.clone(), w.clone(), set.id, set.title.clone(), Some(set));
        },
    );
    let w = lw.clone();
    copy.base
        .enabled
        .set_fn(move |_| w.upgrade().expect("list").sel_value.borrow().is_some());
    let copy_r: ControlRef = copy.clone();
    let (w, tb) = (lw.clone(), tab.clone());
    let delete = ButtonControl::new(
        anchor("LEFT", &copy_r, "RIGHT"),
        rect(4.0, 0.0, 60.0, 18.0),
        "Delete".to_string(),
        move |g| {
            let l = w.upgrade().expect("list");
            let (i, id) = (l.sel_index.get().expect("selIndex"), l.sel_value.borrow().expect("selValue"));
            delete_set(g, tb.clone(), w.clone(), i, id);
        },
    );
    let w = lw.clone();
    delete.base.enabled.set_fn(move |_| {
        let l = w.upgrade().expect("list");
        let n = l.list.borrow().len();
        l.sel_value.borrow().is_some() && n > 1
    });
    let (w, tb) = (lw.clone(), tab.clone());
    let rename = ButtonControl::new(
        anchor("BOTTOMRIGHT", &lr, "TOP"),
        rect(-2.0, -4.0, 60.0, 18.0),
        "Rename".to_string(),
        move |g| {
            let t = tb.upgrade().expect("configTab");
            let id = w.upgrade().expect("list").sel_value.borrow().expect("selValue");
            let title = t.e(|e| e.config_set_title(id));
            rename_set(g, tb.clone(), w.clone(), id, title, None);
        },
    );
    let w = lw.clone();
    rename
        .base
        .enabled
        .set_fn(move |_| w.upgrade().expect("list").sel_value.borrow().is_some());
    let rn_r: ControlRef = rename.clone();
    let (w, tb) = (lw.clone(), tab.clone());
    let new_btn = ButtonControl::new(
        anchor("RIGHT", &rn_r, "LEFT"),
        rect(-4.0, 0.0, 60.0, 18.0),
        "New".to_string(),
        move |g| {
            let t = tb.upgrade().expect("configTab");
            let set = t.e(Engine::config_new_set);
            rename_set(g, tb.clone(), w.clone(), set.id, None, Some(set));
        },
    );
    list.host.add("copy", copy);
    list.host.add("delete", delete);
    list.host.add("rename", rename);
    list.host.add("new", new_btn);
    let done = ButtonControl::new(None, rect(0.0, 260.0, 90.0, 20.0), "Done".to_string(), |_g| main().close_popup());
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert("1".into(), list);
    controls.insert("2".into(), done);
    main().open_popup(g, 370.0, 290.0, "Manage Config Sets", controls, None, None, None, None);
}
