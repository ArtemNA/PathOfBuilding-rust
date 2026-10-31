//! `data.gems` and its lookups (Modules/Data.lua:1085-1155) -- pob-ts `src/data/gems.ts`. Resolution of a saved gem against them:
//! `pob_calc::skills_tab` (R6.2). Checked against LuaJIT (`crates/pob-calc/tests/gemdata.rs`): all 871 gems (with the Vaal AltX/AltY ones),
//! `gemForSkill` -- 37 effects shared by 2-3 gems, all a Vaal gem and its Alt variants set up in one iteration, so the last Alt wins in
//! every process and the converter's map is LuaJIT's -- `gemForBaseName`, `gemsByGameId`. Map order = data order (R13: the `pairs`
//! order of LuaJIT's string-keyed tables differs per process).

use std::sync::OnceLock;

use serde::Deserialize;

use crate::wire::OrderedMap;

/// One `data.gems[id]` entry.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GemEntry {
    pub id: String,
    pub game_id: String,
    pub variant_id: String,
    pub name: String,
    pub base_type_name: Option<String>,
    pub granted_effect_id: String,
    pub secondary_granted_effect_id: Option<String>,
    pub secondary_effect_name: Option<String>,
    /// `gem.tags.fire`, ... -- lowercase tag names, all true when present.
    pub tags: OrderedMap<bool>,
    pub tag_string: String,
    pub req_str: f64,
    pub req_dex: f64,
    pub req_int: f64,
    pub natural_max_level: f64,
    /// Present (true) only on the Vaal gems.
    pub vaal_gem: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gems {
    /// By metadata id.
    pub gems: OrderedMap<GemEntry>,
    /// `data.gemForSkill` re-keyed by grantedEffect id (D4).
    pub gem_for_skill: OrderedMap<String>,
    /// LOWERCASE gem name -> gem id.
    pub gem_for_base_name: OrderedMap<String>,
    /// gameId -> { variantId -> gem id } (D15: upstream walks it with `pairs`).
    pub gems_by_game_id: OrderedMap<OrderedMap<String>>,
    pub gem_granted_effect_id_for_vaal_gem_id: OrderedMap<String>,
    /// Despite the name: VAAL gem id -> base gem id (CalcTools.lua:287).
    pub gem_vaal_gem_id_for_base_gem_id: OrderedMap<String>,
}

pub fn gems() -> &'static Gems {
    static G: OnceLock<Gems> = OnceLock::new();
    G.get_or_init(|| crate::load("gems.json"))
}
