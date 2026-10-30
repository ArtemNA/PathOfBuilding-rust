//! `data.*` rule tables (Modules/Data.lua:429-628 and Data/Misc.lua) -- pob-ts `src/data/rules.ts`. The tables R3 reads are
//! typed; the rest (timeless/crucible/vestigial lists, consumed from R5 on) stay `serde_json::Value`.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::OrderedMap;

/// `data.weaponTypeInfo[type]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeaponTypeInfo {
    pub one_hand: Option<bool>,
    pub melee: Option<bool>,
    pub flag: Option<String>,
    pub label: Option<String>,
}

/// `data.unarmedWeaponData[classId]` -- the converter wrote the 0-based Lua table as an array carrying `classId`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(non_snake_case)]
pub struct UnarmedWeaponData {
    #[serde(rename = "classId")]
    pub class_id: f64,
    #[serde(rename = "type")]
    pub ty: String,
    pub AttackRate: f64,
    pub CritChance: f64,
    pub PhysicalMin: f64,
    pub PhysicalMax: f64,
}

/// `data.nonDamagingAilment[name]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NonDamagingAilment {
    pub associated_type: Option<String>,
    pub alt: Option<bool>,
    pub default: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub precision: Option<f64>,
    pub duration: Option<f64>,
}

/// One `data.jewelRadii[ver][i]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JewelRadius {
    pub inner: f64,
    pub outer: f64,
    pub col: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rules {
    pub weapon_type_info: OrderedMap<WeaponTypeInfo>,
    pub unarmed_weapon_data: Vec<UnarmedWeaponData>,
    pub non_damaging_ailment: OrderedMap<NonDamagingAilment>,
    pub ailment_type_list: Vec<String>,
    pub elemental_ailment_type_list: Vec<String>,
    pub non_damaging_ailment_type_list: Vec<String>,
    pub non_elemental_ailment_type_list: Vec<String>,
    /// `data.highPrecisionMods[name][type]` -> decimal places.
    pub high_precision_mods: OrderedMap<OrderedMap<f64>>,
    pub default_high_precision: f64,
    pub curse_priority: OrderedMap<f64>,
    pub keystones: Vec<String>,
    /// Keyed "3_15" / "3_16" (Data.lua:603-628).
    pub jewel_radii: OrderedMap<Vec<JewelRadius>>,
    pub skill_color_map: Vec<String>,
    pub enchantment_source: Json,
    /// `data.itemTagSpecial[tag][slotName]` -> patterns.
    pub item_tag_special: OrderedMap<OrderedMap<Vec<String>>>,
    pub item_tag_special_exclusion_pattern: OrderedMap<OrderedMap<Vec<String>>>,
    pub timeless_jewel_types: Json,
    pub timeless_jewel_seed_min: Json,
    pub timeless_jewel_seed_max: Json,
    #[serde(rename = "timelessJewelTradeIDs")]
    pub timeless_jewel_trade_ids: Json,
    pub timeless_jewel_additions: f64,
    pub caster_tag_crucible_uniques: Json,
    pub minion_tag_crucible_uniques: Json,
    pub vestigial_unique_base_types: Json,
    pub vestigial_mod_mappings: OrderedMap<String>,
    pub boss_stats: OrderedMap<f64>,
    pub enemy_is_boss_tooltip: String,
    pub power_stat_list: Json,
}

impl Rules {
    /// `data.unarmedWeaponData[classId]` -- by the carried key, never by position (L8).
    pub fn unarmed_weapon_data_for(&self, class_id: f64) -> Option<&UnarmedWeaponData> {
        self.unarmed_weapon_data.iter().find(|r| r.class_id == class_id)
    }
}

pub fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| crate::load("rules.json"))
}
