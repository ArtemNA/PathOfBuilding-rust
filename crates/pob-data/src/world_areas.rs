//! `data.worldAreas` (5606cc64f: Data/WorldAreas.lua, loaded by Modules/Data.lua:1506-1507) -- read only by the Spectre Library's spawn list
//! (MinionListControl.lua:145-174).

use std::sync::OnceLock;

use serde::Deserialize;

use crate::wire::OrderedMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldArea {
    pub name: String,
    pub base_name: String,
    pub description: Option<Vec<String>>,
    pub tags: Vec<String>,
    pub act: f64,
    pub level: f64,
    pub is_map: bool,
    pub monster_varieties: Vec<String>,
    pub boss_varieties: Option<Vec<String>>,
}

/// `data.worldAreas[id]`, in the file's order (the converter's: ids sorted).
pub fn world_areas() -> &'static OrderedMap<WorldArea> {
    static W: OnceLock<OrderedMap<WorldArea>> = OnceLock::new();
    W.get_or_init(|| crate::load("world-areas.json"))
}
