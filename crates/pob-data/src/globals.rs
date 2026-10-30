//! `ModFlag`, `KeywordFlag`, `SkillType`, `colorCodes` (Data/Global.lua) -- pob-ts `src/data/flags.ts` reads the same file.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::wire::OrderedMap;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Globals {
    #[serde(rename = "ModFlag")]
    pub mod_flag: OrderedMap<f64>,
    #[serde(rename = "KeywordFlag")]
    pub keyword_flag: OrderedMap<f64>,
    #[serde(rename = "SkillType")]
    pub skill_type: OrderedMap<f64>,
    #[serde(rename = "colorCodes")]
    pub color_codes: OrderedMap<String>,
    #[serde(rename = "defaultColorCodes")]
    pub default_color_codes: OrderedMap<String>,
}

pub fn globals() -> &'static Globals {
    static G: OnceLock<Globals> = OnceLock::new();
    G.get_or_init(|| crate::load("globals.json"))
}
