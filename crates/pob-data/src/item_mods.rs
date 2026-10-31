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
