//! Classes/ConfigTab.lua's data half (R3.5, CALC-ARCH §6): the active config set (`input`/`placeholder`/`customModsList`),
//! `enemyLevel`, `NewConfigSet` (:1280-1300), `UpdateLevel` (:1118-1128), the placeholder callback (:305-307 via
//! EditControl.lua:111-116) and `BuildModList` (:1130-1203). Executable spec: pob-ts `src/config/state.ts`.
//!
//! The tab's UI half is modelled only as far as the `apply` closures reach into it (`varControls[x]:SetPlaceholder`,
//! `:SelByValue`, `.selIndex`, `.enabled`, `configTab:UpdateLevel()`): [`ConfigControls`] plus the methods on
//! [`ConfigState`]. Placeholder writes land in the active set's table mid-loop -- `enemyIsBoss` (varList index 652) changes
//! what `enemyArmour`/`enemyEvasion`/`enemy*Resist` read a few iterations later. Do not split that into two passes.
//!
//! Registry of the `apply` closures: [`apply`] (R3-ARCH §2.5) -- 92 hand-ported + 447 generated (`apply_gen.rs`, R3.5b) = all 539.

pub mod apply;
#[rustfmt::skip]
mod apply_gen;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use indexmap::IndexMap;
use pob_data::config_options::config_options;
use pob_data::wire::Scalar;
use pob_lua::number::min2;
use pob_lua::{Capture, Repl, gmatch, gsub, match_, tonumber, tostring_num};
use pob_mods::format::set_source;
use pob_mods::{LuaError, LuaResult, ModList};

use crate::load::active_config;
use crate::model::{BlockText, BuildModel, ConfigSetModel, CustomModsBlockModel, InputValue};

pub use apply::{
    APPLY_REGISTRY, ApplyCtx, ApplyEntry, HAND_PORTED, MAP_AFFIX_APPLY, MapAffixEntry, map_affix_drift, map_affixes_with_apply,
    registry_drift,
};
pub use apply_gen::GENERATED;

/// A config `input`/`placeholder` value: Lua number, string or boolean (placeholders are never booleans).
pub type ConfigValue = InputValue;

/// `ConfigTab.varControls[var]` -- the sliver the apply closures write. The controls live on the TAB, not on the set, so
/// they survive a `BuildModList`; `presetBossSkills`' "None" branch depends on that (ConfigOptions.lua:2344-2349).
/// Absent = the `Control` defaults (`enabled = true`, Control.lua:60; `selIndex` = what `UpdateControls` syncs, :1017).
#[derive(Clone, Debug, Default)]
pub struct ConfigControls {
    pub enabled: IndexMap<String, bool>,
    /// 1-based (L8).
    pub sel_index: IndexMap<String, usize>,
}

/// `configSets[activeConfigSetId]` + `ConfigTab.enemyLevel` + the persistent control bits.
#[derive(Clone, Debug)]
pub struct ConfigState {
    /// `input`: the ConfigOptions defaults with the XML's own values on top.
    pub input: IndexMap<String, ConfigValue>,
    /// `placeholder`; a `tonumber(...) == nil` write DELETES the key (L3).
    pub placeholder: IndexMap<String, ConfigValue>,
    pub custom_mods_list: Vec<CustomModsBlockModel>,
    /// `ConfigTab.enemyLevel` -- 1 until the first `UpdateLevel` (ConfigTab.lua:145).
    pub enemy_level: f64,
    pub controls: ConfigControls,
    /// How the custom-modifier lines are read (R5.4): `Full` (desktop, default) or `CacheOnly` (pob-ts).
    pub parse_mode: pob_parser::ParseMode,
}

/// The output of `BuildModList`: `configTab.modList`/`.enemyModList` plus what was consciously not done.
#[derive(Debug, Default)]
pub struct ConfigModLists {
    pub mod_list: ModList,
    pub enemy_mod_list: ModList,
    /// Every var whose upstream `apply` would have fired but has no registry entry:
    /// `Modules/ConfigOptions.lua:<from>-<to> apply '<var>'`.
    pub unimplemented: Vec<String>,
    /// Input keys ConfigOptions has no `varData` for -- BuildModList silently skips them (igniteMode, ...).
    pub ignored_legacy: Vec<String>,
    /// Off-target `SetPlaceholder` calls (no EditControl upstream).
    pub warnings: Vec<String>,
}

/// `varList` lookups, built once: name -> index, and the registry entry for each index.
struct VarIndex {
    by_name: HashMap<&'static str, usize>,
    entry: Vec<Option<&'static ApplyEntry>>,
}

fn var_index() -> &'static VarIndex {
    static I: OnceLock<VarIndex> = OnceLock::new();
    I.get_or_init(|| {
        let vars = &config_options().vars;
        let reg: HashMap<&str, &'static ApplyEntry> = APPLY_REGISTRY.iter().map(|e| (e.var, e)).collect();
        let mut by_name = HashMap::new();
        let mut entry = Vec::with_capacity(vars.len());
        for (i, v) in vars.iter().enumerate() {
            match &v.var {
                Some(name) => {
                    by_name.entry(name.as_str()).or_insert(i);
                    entry.push(reg.get(name.as_str()).copied());
                }
                None => entry.push(None),
            }
        }
        VarIndex { by_name, entry }
    })
}

/// The `varData` of a var name.
fn var_data(name: &str) -> Option<&'static pob_data::config_options::ConfigVar> {
    var_index().by_name.get(name).map(|&i| &config_options().vars[i])
}

pub(crate) fn scalar(s: &Scalar) -> ConfigValue {
    match s {
        Scalar::Num(n) => InputValue::Num(*n),
        Scalar::Str(t) => InputValue::Str(t.clone()),
        Scalar::Bool(b) => InputValue::Bool(*b),
    }
}

/// Lua truthiness of a present value (L4): only `false` is false; `0` and `""` are true.
pub(crate) fn truthy(v: Option<&ConfigValue>) -> bool {
    !matches!(v, None | Some(InputValue::Bool(false)))
}

/// Lua `tostring(v)`.
pub(crate) fn lua_tostring(v: &ConfigValue) -> String {
    match v {
        InputValue::Num(n) => tostring_num(*n),
        InputValue::Str(s) => s.clone(),
        InputValue::Bool(b) => b.to_string(),
    }
}

/// Lua `t[k] = v` where `v` may be nil.
fn set_or_remove(t: &mut IndexMap<String, ConfigValue>, k: &str, v: Option<ConfigValue>) {
    match v {
        Some(v) => {
            t.insert(k.to_owned(), v);
        }
        None => {
            t.shift_remove(k);
        }
    }
}

impl Default for ConfigState {
    fn default() -> Self {
        ConfigState::new()
    }
}

impl ConfigState {
    /// `ConfigTab:NewConfigSet` (:1280-1300) + the ctor's `enemyLevel = 1` (:145) and empty `varControls`: every var gets
    /// `input = defaultState`, `placeholder = defaultPlaceholderState` (nil removes), and a list with `defaultIndex` gets
    /// `input = list[defaultIndex].val` -- in varList order.
    pub fn new() -> ConfigState {
        let mut input = IndexMap::new();
        let mut placeholder = IndexMap::new();
        for v in &config_options().vars {
            let Some(name) = &v.var else { continue };
            set_or_remove(&mut input, name, v.default_state.as_ref().map(scalar));
            set_or_remove(&mut placeholder, name, v.default_placeholder_state.as_ref().map(scalar));
            if let Some(di) = v.default_index {
                let list = v.list.as_ref().expect("ConfigOptions: defaultIndex without list");
                let val = &list[di as usize - 1].val;
                input.insert(name.clone(), scalar(val));
            }
        }
        ConfigState {
            input,
            placeholder,
            custom_mods_list: vec![CustomModsBlockModel::default_block()],
            enemy_level: 1.0,
            controls: ConfigControls::default(),
            parse_mode: pob_parser::ParseMode::Full,
        }
    }

    /// One `configSets[id]` as `ConfigTab:Load` leaves it: `NewConfigSet` (defaults, in varList order) with the XML's writes
    /// on top in document order -- a nil write (failed `tonumber`) deletes the key, a default included.
    pub fn from_set(set: &ConfigSetModel) -> ConfigState {
        let mut state = ConfigState::new();
        for (k, v) in &set.input {
            set_or_remove(&mut state.input, k, v.clone());
        }
        for (k, v) in &set.placeholder {
            set_or_remove(&mut state.placeholder, k, v.map(InputValue::Num));
        }
        state.custom_mods_list = set.custom_mods_list.clone();
        state
    }

    /// The active set right after `Build:Load` (`ConfigTab:Load` + `SetActiveConfigSet`, before its `BuildModList`).
    ///
    /// D17 (bug-compat): Build.lua:649-651 copies `bandit`/`pantheonMajorGod`/`pantheonMinorGod` from `<Build>` into
    /// `configTab.input` BEFORE `ConfigTab:Load`, which replaces the sets -- so with a `<Config>` element (all six goldens)
    /// the three are lost. Without any `<Config>` element Load never runs and they survive in the ctor's set.
    pub fn from_build(model: &BuildModel) -> ConfigState {
        if !model.has_config_section {
            let mut state = ConfigState::new();
            state.input.insert("bandit".into(), InputValue::Str(model.bandit.clone()));
            state
                .input
                .insert("pantheonMajorGod".into(), InputValue::Str(model.pantheon_major_god.clone()));
            state
                .input
                .insert("pantheonMinorGod".into(), InputValue::Str(model.pantheon_minor_god.clone()));
            return state;
        }
        ConfigState::from_set(active_config(model))
    }

    /// `ConfigTab:UpdateLevel` (:1118-1128): `input.enemyLevel and input.enemyLevel > 0` (L4: a stored 0 falls through),
    /// then the placeholder, then the character level, each capped by `data.misc.MaxEnemyLevel`.
    pub fn update_level(&mut self, character_level: f64) -> LuaResult<()> {
        let max_level = pob_data::constants::constants().misc["MaxEnemyLevel"];
        let positive = |v: Option<&ConfigValue>, what: &str| -> LuaResult<Option<f64>> {
            match v {
                None | Some(InputValue::Bool(false)) => Ok(None),
                Some(InputValue::Num(n)) => Ok((*n > 0.0).then_some(*n)),
                Some(other) => Err(LuaError(format!(
                    "Classes/ConfigTab.lua:1121: attempt to compare number with {} ({what}.enemyLevel)",
                    if matches!(other, InputValue::Str(_)) { "string" } else { "boolean" }
                ))),
            }
        };
        self.enemy_level = if let Some(n) = positive(self.input.get("enemyLevel"), "input")? {
            min2(max_level, n)
        } else if let Some(n) = positive(self.placeholder.get("enemyLevel"), "placeholder")? {
            min2(max_level, n)
        } else {
            min2(max_level, character_level)
        };
        Ok(())
    }

    /// `varControls[var]:SetPlaceholder(v, true)` -> `changeFunc(tostring(v), true)` -> the ConfigTab callback (:305-307):
    /// `count|integer|countAllowZero|float` -> `placeholder[var] = tonumber(buf)` (nil deletes); `text` ->
    /// `tostring(buf)`. Other types have no EditControl (upstream: "attempt to call a nil value") -> a warning.
    pub fn set_placeholder(&mut self, var: &str, v: &ConfigValue, warnings: &mut Vec<String>) {
        let buf = lua_tostring(v);
        match var_data(var).and_then(|d| d.ty.as_deref()) {
            Some("count" | "integer" | "countAllowZero" | "float") => {
                set_or_remove(&mut self.placeholder, var, tonumber(&buf).map(InputValue::Num));
            }
            Some("text") => {
                self.placeholder.insert(var.to_owned(), InputValue::Str(buf));
            }
            ty => warnings.push(format!(
                "SetPlaceholder on '{var}' (type {}): no EditControl, ignored",
                ty.unwrap_or("nil")
            )),
        }
    }

    /// `varControls[var].selIndex`: a `SelByValue` an apply closure performed wins; otherwise what `UpdateControls` (:1017)
    /// syncs from `input` before `SetActiveConfigSet` runs BuildModList -- the index of `input[var]`, else the ctor's
    /// `defaultIndex` (:678). `None` when the var is not a dropdown.
    pub fn sel_index(&self, var: &str) -> Option<usize> {
        if let Some(&i) = self.controls.sel_index.get(var) {
            return Some(i);
        }
        let d = var_data(var)?;
        let list = d.list.as_ref()?;
        let cur = self.input.get(var);
        match list.iter().position(|e| cur.is_some_and(|c| *c == scalar(&e.val))) {
            Some(i) => Some(i + 1),
            None => d.default_index.map(|x| x as usize),
        }
    }

    /// `varControls[var]:SelByValue(value, "val")` (DropDownControl.lua:136-150 -- a miss leaves selIndex alone).
    pub fn sel_by_value(&mut self, var: &str, value: &ConfigValue) {
        let Some(list) = var_data(var).and_then(|d| d.list.as_ref()) else {
            return;
        };
        if let Some(i) = list.iter().position(|e| scalar(&e.val) == *value) {
            self.controls.sel_index.insert(var.to_owned(), i + 1);
        }
    }

    /// `varControls[var].list[i].val`, 1-based.
    pub fn list_value_at(&self, var: &str, index: Option<usize>) -> Option<ConfigValue> {
        let i = index?;
        let list = var_data(var)?.list.as_ref()?;
        list.get(i.checked_sub(1)?).map(|e| scalar(&e.val))
    }

    pub fn control_enabled(&self, var: &str) -> bool {
        self.controls.enabled.get(var).copied().unwrap_or(true)
    }

    pub fn set_control_enabled(&mut self, var: &str, value: bool) {
        self.controls.enabled.insert(var.to_owned(), value);
    }

    /// `varControls[var].placeholder` -- what `UpdateControls`/`SetPlaceholder` last stringified, else the ctor's
    /// `defaultPlaceholderState` (:679). The string form is coerced back by the arithmetic that reads it (L6).
    pub fn control_placeholder(&self, var: &str) -> Option<ConfigValue> {
        self.placeholder
            .get(var)
            .cloned()
            .or_else(|| var_data(var)?.default_placeholder_state.as_ref().map(scalar))
    }

    /// `ConfigTab:BuildModList` (:1130-1203), verbatim: `UpdateLevel()` first, then `ipairs(varList)` (mod order = the
    /// ConfigOptions declaration order, L10), then the custom-modifier blocks (or the legacy `input.customMods`).
    pub fn build_mod_list(&mut self, character_level: f64) -> LuaResult<ConfigModLists> {
        let mut mod_list = ModList::default();
        let mut enemy_mod_list = ModList::default();
        let mut unimplemented = Vec::new();
        let mut warnings = Vec::new();
        self.update_level(character_level)?;

        let idx = var_index();
        for (i, var_data) in config_options().vars.iter().enumerate() {
            let (Some(name), Some(closure)) = (&var_data.var, &var_data.apply) else {
                continue;
            };
            let Some(val) = self.select_value(name, var_data.ty.as_deref()) else {
                continue;
            };
            let Some(entry) = idx.entry[i] else {
                unimplemented.push(format!("Modules/ConfigOptions.lua:{}-{} apply '{name}'", closure.from, closure.to));
                continue;
            };
            let mut ctx = ApplyCtx {
                mod_list: &mut mod_list,
                enemy_mod_list: &mut enemy_mod_list,
                tab: self,
                character_level,
                warnings: &mut warnings,
            };
            (entry.apply)(&val, &mut ctx)?;
        }

        // Custom Modifier groups (:1163-1186): `block.enabled ~= false and block.text and #block.text > 0`, then
        // `block.text:gmatch(...)` -- on whatever Lua value `text` holds (BlockText).
        let mut has_block_text = false;
        for block in &self.custom_mods_list {
            if !block.enabled {
                continue;
            }
            match &block.text {
                BlockText::Str(text) if text.is_empty() => {}
                BlockText::Str(text) => {
                    has_block_text = true;
                    add_custom_lines(&mut mod_list, text, &format!("Custom:{}", block.title), self.parse_mode);
                }
                BlockText::Bool(false) => {}
                BlockText::Num(_) | BlockText::Bool(_) => {
                    return Err(LuaError(format!(
                        "Classes/ConfigTab.lua:1167: attempt to get length of field 'text' (a {} value)",
                        if matches!(block.text, BlockText::Num(_)) {
                            "number"
                        } else {
                            "boolean"
                        }
                    )));
                }
                // `#` of the element table = its child count.
                BlockText::Element(e) if e.children.is_empty() => {}
                BlockText::Element(_) => {
                    return Err(LuaError(
                        "Classes/ConfigTab.lua:1169: attempt to call method 'gmatch' (a nil value)".into(),
                    ));
                }
            }
        }
        // Fallback for tests/headless (:1187-1202). `ConfigTab:Load` nils `input.customMods` of every listed set (:934).
        if !has_block_text {
            match self.input.get("customMods") {
                Some(InputValue::Str(legacy)) if !legacy.is_empty() => add_custom_lines(&mut mod_list, legacy, "Custom", self.parse_mode),
                Some(v @ (InputValue::Num(_) | InputValue::Bool(true))) => {
                    return Err(LuaError(format!(
                        "Classes/ConfigTab.lua:1187: attempt to get length of field 'customMods' (a {} value)",
                        if matches!(v, InputValue::Num(_)) { "number" } else { "boolean" }
                    )));
                }
                _ => {}
            }
        }

        let ignored_legacy = self
            .input
            .keys()
            .filter(|k| !idx.by_name.contains_key(k.as_str()))
            .cloned()
            .collect();
        Ok(ConfigModLists {
            mod_list,
            enemy_mod_list,
            unimplemented,
            ignored_legacy,
            warnings,
        })
    }

    /// The `if input[var]` / `~= 0` gate of BuildModList (:1138-1157) by varData type; `None` = the closure does not fire.
    fn select_value(&self, name: &str, ty: Option<&str>) -> Option<ConfigValue> {
        let input = self.input.get(name);
        match ty {
            // `varData.apply(true, ...)` -- the literal true, not input[var] (:1140).
            Some("check") => truthy(input).then_some(InputValue::Bool(true)),
            Some(t @ ("count" | "integer" | "countAllowZero" | "float")) => {
                let allow_zero = t == "countAllowZero";
                // `x and (x ~= 0 or allowZero)`: Lua `~=` across types is true ("0" ~= 0).
                let fires = |v: Option<&ConfigValue>| truthy(v) && (allow_zero || !matches!(v, Some(InputValue::Num(n)) if *n == 0.0));
                if fires(input) {
                    input.cloned()
                } else {
                    let p = self.placeholder.get(name);
                    if fires(p) { p.cloned() } else { None }
                }
            }
            Some("list" | "text") => truthy(input).then(|| input.cloned()).flatten(),
            _ => None,
        }
    }
}

/// `StripEscapes` (_SimpleGraphic.def.lua:295): `text:gsub("%^%d", ""):gsub("%^x%x%x%x%x%x%x", "")` (L13).
fn strip_escapes(text: &[u8]) -> Vec<u8> {
    let a = gsub(text, b"%^%d", Repl::Str(b""), None).expect("StripEscapes pattern").0;
    gsub(&a, b"%^x%x%x%x%x%x%x", Repl::Str(b""), None).expect("StripEscapes pattern").0
}

/// `for line in text:gmatch("([^\n]*)\n?")` -> `StripEscapes(line):match("^%s*(.-)%s*$")` -> `modLib.parseMod` ->
/// `if mods and not extra` (an `extra` of "" is truthy, L4) -> `modLib.setSource` + `AddMod` (:1166-1183).
fn add_custom_lines(mod_list: &mut ModList, text: &str, source: &str, mode: pob_parser::ParseMode) {
    let text = text.as_bytes();
    for caps in gmatch(text, b"([^\n]*)\n?") {
        let caps = caps.expect("BuildModList line pattern");
        let Some(Capture::Str { start, end }) = caps.get(0) else { continue };
        let stripped = strip_escapes(&text[start..end]);
        let trimmed = match match_(&stripped, b"^%s*(.-)%s*$", None).expect("trim pattern") {
            Some(c) => match c.get(0) {
                Some(Capture::Str { start, end }) => stripped[start..end].to_vec(),
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
        let line = String::from_utf8_lossy(&trimmed);
        let parsed = pob_parser::parse_mod_line(&line, false, mode);
        if let (Some(mods), None) = (parsed.mod_list, parsed.extra) {
            for mut m in mods {
                set_source(&mut m, source);
                mod_list.mods.push(Rc::new(m));
            }
        }
    }
}

/// `ConfigTab:ImportCalcSettings` (:1205-1240): Build.lua:685 reaches it only when `next(configTab.input) == nil`, and
/// `NewConfigSet` has seeded defaults by then -- unreachable on any build this port loads.
pub const IMPORT_CALC_SETTINGS_UNIMPLEMENTED: &str =
    "Classes/ConfigTab.lua:1205-1240 ImportCalcSettings (unreachable: input is never empty)";
