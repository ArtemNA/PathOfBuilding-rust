//! `data.itemMods.<Category>` (Data/ModItem*.lua, Data/ModFoulborn.lua ...) -- only the mod LINES, the array part of each
//! mod table (`table.concat(mod, " ")` / `ipairs(mod)` in Item.lua:1551-1600). The other fields are not read by pob-items.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::wire::OrderedMap;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ItemModLines {
    #[serde(default)]
    pub lines: Vec<String>,
}

/// `data.itemMods.ItemExclusive`.
pub fn item_exclusive() -> &'static OrderedMap<ItemModLines> {
    static P: OnceLock<OrderedMap<ItemModLines>> = OnceLock::new();
    P.get_or_init(|| crate::load("item-mods/ItemExclusive.json"))
}

/// `data.itemMods.Foulborn`.
pub fn foulborn() -> &'static OrderedMap<ItemModLines> {
    static P: OnceLock<OrderedMap<ItemModLines>> = OnceLock::new();
    P.get_or_init(|| crate::load("item-mods/Foulborn.json"))
}

/// One `data.itemMods.<Category>[modId]`: only what is read (`tradeHashes` is not). Deserialized directly: test `affix_mods.rs` of pob-items keeps it equal to [`AffixMod::from_json`] over every data file.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AffixMod {
    /// The key of the table, not a field of the JSON.
    #[serde(skip)]
    pub id: String,
    pub lines: Vec<String>,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub affix: String,
    pub group: Option<String>,
    pub level: f64,
    pub stat_order: Vec<f64>,
    pub mod_tags: Vec<String>,
    pub tags: Vec<String>,
    pub weight_key: Vec<String>,
    pub weight_val: Vec<f64>,
    pub weight_multiplier_key: Vec<String>,
    pub weight_multiplier_val: Vec<f64>,
}

fn strs(v: &serde_json::Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect()
}

fn nums(v: &serde_json::Value, k: &str) -> Vec<f64> {
    v.get(k)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_f64)
        .collect()
}

impl AffixMod {
    /// The JSON the data converter wrote.
    pub fn from_json(id: &str, v: &serde_json::Value) -> AffixMod {
        let s = |k: &str| v.get(k).and_then(serde_json::Value::as_str).map(str::to_owned);
        AffixMod {
            id: id.to_owned(),
            lines: strs(v, "lines"),
            ty: s("type"),
            affix: s("affix").unwrap_or_default(),
            group: s("group"),
            level: v.get("level").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            stat_order: nums(v, "statOrder"),
            mod_tags: strs(v, "modTags"),
            tags: strs(v, "tags"),
            weight_key: strs(v, "weightKey"),
            weight_val: nums(v, "weightVal"),
            weight_multiplier_key: strs(v, "weightMultiplierKey"),
            weight_multiplier_val: nums(v, "weightMultiplierVal"),
        }
    }
}
