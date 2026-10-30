//! `data.pantheons` (Data/Pantheons.lua) -- pob-ts `src/data/pantheons.ts`. Only the soul LINES are used: PantheonTools.lua
//! parses them with `modLib.parseMod` (pob-parser) and stamps the first soul's name as the source.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::OrderedMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PantheonSoulMod {
    pub line: String,
    /// The numbers the converter pulled out of the line (unused by the calc).
    pub value: Option<Json>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PantheonSoul {
    pub name: String,
    pub mods: Vec<PantheonSoulMod>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pantheon {
    pub is_major_god: bool,
    pub souls: Vec<PantheonSoul>,
}

/// `data.pantheons[godName]`.
pub fn pantheons() -> &'static OrderedMap<Pantheon> {
    static P: OnceLock<OrderedMap<Pantheon>> = OnceLock::new();
    P.get_or_init(|| crate::load("pantheons.json"))
}
