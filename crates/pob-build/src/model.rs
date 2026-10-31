//! `BuildModel` -- every attribute the Lua tab `Load`s read from a build XML (CALC-ARCH §2), with Lua-faithful types; the
//! same shape as pob-ts `src/build/model.ts` (serialized field for field, `None` = absent: the R3.2 fixture compares the two).
//! Gem/item/tree resolution against game data is R3.3-R3.5, not here.

use indexmap::IndexMap;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use crate::xml::XmlElement;

/// A Lua table keyed by numbers (`jewels[nodeId] = itemId`, `masteryEffects[mastery] = effect`): insertion order, a repeated
/// key overwrites in place. Serialized with the keys as JS `String(n)` (integers as digits), like pob-ts' `Record<number, ..>`.
#[derive(Clone, Debug, PartialEq)]
pub struct NumMap<V>(pub Vec<(f64, V)>);

impl<V> Default for NumMap<V> {
    fn default() -> Self {
        NumMap(Vec::new())
    }
}

impl<V> NumMap<V> {
    /// `t[k] = v`.
    pub fn set(&mut self, k: f64, v: V) {
        match self.0.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k, v)),
        }
    }

    /// `t[k]`.
    pub fn get(&self, k: f64) -> Option<&V> {
        self.0.iter().find(|(x, _)| *x == k).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A numeric table key as JS `String(n)` -- digits for the integers ids are; anything else through Lua `tostring`.
fn key_string(k: f64) -> String {
    if k.fract() == 0.0 && k.abs() < 9_007_199_254_740_992.0 {
        (k as i64).to_string()
    } else {
        pob_lua::tostring_num(k)
    }
}

impl<V: Serialize> Serialize for NumMap<V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(&key_string(*k), v)?;
        }
        m.end()
    }
}

/// An insertion-ordered string-keyed table as a JSON object (indexmap has no serde feature in this workspace).
fn ser_map<V: Serialize, S: Serializer>(m: &IndexMap<String, V>, s: S) -> Result<S::Ok, S::Error> {
    let mut out = s.serialize_map(Some(m.len()))?;
    for (k, v) in m {
        out.serialize_entry(k, v)?;
    }
    out.end()
}

/// A config/calcs input value: `number | string | boolean`.
#[derive(Clone, Debug, PartialEq)]
pub enum InputValue {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Serialize for InputValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            InputValue::Num(n) => s.serialize_f64(*n),
            InputValue::Str(t) => s.serialize_str(t),
            InputValue::Bool(b) => s.serialize_bool(*b),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildModel {
    /// Build.lua:981 `tonumber(level) or 1`.
    pub level: f64,
    /// :977 `or legacyTargetVersion` ("2_6").
    pub target_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_mode: Option<String>,
    pub character_level_auto_mode: bool,
    /// `<Build>` attributes only -- a UI cache; the truth is the active spec.
    pub class_name: String,
    pub ascend_class_name: String,
    /// :983-985, default "None". D17: the calc never sees these when the file has a `<Config>` -- ConfigTab:Load resets
    /// `input` after Build.lua:650 copied them; that loss is reproduced by the config state (R3.5), not here (same as pob-ts).
    pub bandit: String,
    pub pantheon_major_god: String,
    pub pantheon_minor_god: String,
    /// :986 `tonumber(mainSkillIndex) or tonumber(mainSocketGroup) or 1`.
    pub main_socket_group: f64,
    /// :989-992 `<Spectre id>`. Upstream also requires `data.minions[id]` -- no minion data in R3, so every id is kept
    /// verbatim (pob-ts' simplification, goldens-inert).
    pub spectres: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeless_data: Option<TimelessDataModel>,
    pub tree: TreeTabModel,
    pub items: Vec<ItemModel>,
    pub item_order: Vec<f64>,
    pub item_sets: Vec<ItemSetModel>,
    pub active_item_set: f64,
    pub skills: SkillsModel,
    pub config: ConfigModel,
    /// CalcsTab.lua:163-180 `<Calcs><Input>`.
    #[serde(serialize_with = "ser_map")]
    pub calcs_input: IndexMap<String, InputValue>,
    /// Top-level element names this loader does not model (Import/Notes/TreeView/...), in document order.
    pub unknown_sections: Vec<String>,
    /// Whether the file has a `<Config>` element at all -- D17: only without one do `bandit`/pantheons reach the calc.
    /// pob-ts has no such flag (MINEFIELD D17 asks for it); not part of the compared form.
    #[serde(skip)]
    pub has_config_section: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelessDataModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jewel_type_id: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conqueror_type_id: Option<f64>,
    pub devotion_variant1: f64,
    pub devotion_variant2: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jewel_socket_id: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_weight_mode_idx: Option<f64>,
    pub socket_filter: bool,
    pub socket_allocate: bool,
    pub socket_filter_distance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_list: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_list_fallback: Option<String>,
}

/// TreeTab.lua:500-527: every loaded `<Spec>`, 1-based `activeSpec` (clamped at read time, :526/:551).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeTabModel {
    pub specs: Vec<SpecModel>,
    pub active_spec: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecOverrideModel {
    pub node_id: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_effect_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dn: Option<String>,
    #[serde(serialize_with = "ser_map")]
    pub attrib: IndexMap<String, String>,
}

/// PassiveSpec.lua:103-195. `nodes`/`class_id`/`ascend_class_id` are 0/empty for a legacy URL-only spec (no `nodes`
/// attribute) -- decoding `url` is tree work (R3.3).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub tree_version: String,
    pub cluster_hash_format_version: f64,
    pub class_id: f64,
    pub ascend_class_id: f64,
    pub secondary_ascend_class_id: f64,
    /// hashList, order as written (L9: empty `nodes` -> []).
    pub nodes: Vec<f64>,
    /// `{mastery,effect}` pairs.
    pub mastery_effects: NumMap<f64>,
    /// nodeId -> itemId, itemId > 0 only (:128-133).
    pub jewels: NumMap<f64>,
    /// Legacy `<URL>` text, kept even when `nodes` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Tattoos (R5) -- attributes verbatim.
    pub overrides: Vec<SpecOverrideModel>,
}

/// ItemsTab.lua:1222-1266. `raw` is the `<Item>` text verbatim (Item.ParseRaw is R3.4).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemModel {
    pub id: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_alt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_alt2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_alt3: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_alt4: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_alt5: Option<f64>,
    pub raw: String,
    pub mod_ranges: Vec<ModRangeModel>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModRangeModel {
    pub id: f64,
    pub range: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSetSlotModel {
    pub item_id: f64,
    pub active: bool,
    #[serde(rename = "pbURL", skip_serializing_if = "Option::is_none")]
    pub pb_url: Option<String>,
}

/// ItemsTab.lua:1280-1296 (+ legacy `<Slot>` folded into set 1 when there is no `<ItemSet>`).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSetModel {
    pub id: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub use_second_weapon_set: bool,
    /// By slot name, e.g. "Weapon 1".
    #[serde(serialize_with = "ser_map")]
    pub slots: IndexMap<String, ItemSetSlotModel>,
    #[serde(rename = "socketIdURLs")]
    pub socket_id_urls: NumMap<String>,
}

/// SkillsTab.lua:440-521 LoadSkill. `enabled`/`enable_global1` default true when the attribute is absent (L4).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GemModel {
    pub name_spec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gem_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_id: Option<String>,
    pub enabled: bool,
    pub enable_global1: bool,
    pub enable_global2: bool,
    pub count: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_part: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_part_calcs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_stage_count: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_stage_count_calcs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_mine_count: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_mine_count_calcs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_minion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_minion_calcs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_minion_item_set: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_minion_item_set_calcs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_minion_skill: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_minion_skill_calcs: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketGroupModel {
    pub enabled: bool,
    #[serde(rename = "includeInFullDPS")]
    pub include_in_full_dps: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_count: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub main_active_skill: f64,
    pub main_active_skill_calcs: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imbued_support: Option<String>,
    pub gems: Vec<GemModel>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SkillSetModel {
    pub id: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub groups: Vec<SocketGroupModel>,
}

/// SkillsTab.lua:523-570. `sort_gems_by_dps`/`show_legacy_gems` stay `None` when the XML omits them (single-shot load,
/// no earlier session state to keep -- as pob-ts).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsModel {
    /// normalMaximum | corruptedMaximum | awakenedMaximum | characterLevel | levelOne.
    pub default_gem_level: String,
    pub default_gem_quality: f64,
    #[serde(rename = "sortGemsByDPS", skip_serializing_if = "Option::is_none")]
    pub sort_gems_by_dps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_legacy_gems: Option<bool>,
    pub show_support_gem_types: String,
    #[serde(rename = "sortGemsByDPSField")]
    pub sort_gems_by_dps_field: String,
    pub sets: Vec<SkillSetModel>,
    pub active_skill_set: f64,
}

/// A custom-modifier block's `text` (ConfigTab.lua:893-897/:911-915 `node[1] or ""`, :926-929 legacy `input.customMods`):
/// whatever Lua value lands there -- the element's FIRST child (a string, or an element table) or the legacy input.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockText {
    Str(String),
    Num(f64),
    Bool(bool),
    /// `node[1]` was an element: Lua keeps the table.
    Element(XmlElement),
}

impl BlockText {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            BlockText::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl Serialize for BlockText {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            BlockText::Str(t) => s.serialize_str(t),
            BlockText::Num(n) => s.serialize_f64(*n),
            BlockText::Bool(b) => s.serialize_bool(*b),
            // Not a pob-ts shape (pob-ts concatenates the text); never on the goldens.
            BlockText::Element(e) => s.serialize_str(&e.text()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CustomModsBlockModel {
    pub title: String,
    pub enabled: bool,
    pub text: BlockText,
}

impl CustomModsBlockModel {
    /// `{ title = "Default", enabled = true, text = "" }`.
    pub fn default_block() -> CustomModsBlockModel {
        CustomModsBlockModel {
            title: "Default".into(),
            enabled: true,
            text: BlockText::Str(String::new()),
        }
    }
}

/// `t[k] = v` writes with `v` possibly nil: serialized as the pob-ts form (only the non-nil writes).
fn ser_writes<V: Serialize, S: Serializer>(m: &IndexMap<String, Option<V>>, s: S) -> Result<S::Ok, S::Error> {
    let mut out = s.serialize_map(None)?;
    for (k, v) in m {
        if let Some(v) = v {
            out.serialize_entry(k, v)?;
        }
    }
    out.end()
}

/// One `configSets[id]` of ConfigTab.lua:839-938 as the XML wrote it: `input`/`placeholder` are the writes of
/// `setInputAndPlaceholder` in order (`None` = a nil write: `tonumber` failed, which DELETES the `NewConfigSet` default),
/// applied on top of the ConfigOptions defaults by `ConfigState::from_set`. `title` is `None` only for the ctor's set
/// (`NewConfigSet(1)`, ConfigTab.lua:147) of a file without `<Config>`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetModel {
    pub id: f64,
    pub title: Option<String>,
    #[serde(serialize_with = "ser_writes")]
    pub input: IndexMap<String, Option<InputValue>>,
    #[serde(serialize_with = "ser_writes")]
    pub placeholder: IndexMap<String, Option<f64>>,
    pub custom_mods_list: Vec<CustomModsBlockModel>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigModel {
    /// `configSets`, in creation order; a repeated id replaces the set in place (`NewConfigSet`).
    pub sets: Vec<ConfigSetModel>,
    /// `activeConfigSetId` as `SetActiveConfigSet` (:1337-1354) resolves it.
    pub active_config_set: f64,
    /// `configSetOrderList` as `(index, id)` pairs sorted by index -- sparse (`[index] = id` with the `<ConfigSet>`'s
    /// position among ALL children of `<Config>`, :906). Not part of the pob-ts form.
    #[serde(skip)]
    pub set_order_list: Vec<(f64, f64)>,
}

/// `getStats()._meta` as the model alone can answer it (raw XML counts; tree/skill resolution supersedes in R3.6).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub alloc_nodes: f64,
    pub items: f64,
    pub socket_groups: f64,
    pub main_socket_group: f64,
    pub tree_version: String,
    pub class_name: String,
    pub ascend_name: String,
}
