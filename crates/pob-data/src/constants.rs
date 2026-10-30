//! `data.misc`, `data.characterConstants`, `data.monsterConstants`, `data.gameConstants` and the per-level monster tables
//! (Modules/Data.lua:75-119) -- pob-ts `src/data/constants.ts`.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::wire::OrderedMap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Constants {
    /// `data.misc` -- 60 scalar tuning constants.
    pub misc: OrderedMap<f64>,
    /// 89 `base_*` player stats.
    pub character_constants: OrderedMap<f64>,
    /// 57 monster stats.
    pub monster_constants: OrderedMap<f64>,
    /// 42 game-wide numbers.
    pub game_constants: OrderedMap<f64>,
    /// The per-monster-level tables: 1-based Lua sequences of length 100 -- read with `monster_table_at` (L8).
    pub monster_accuracy_table: Vec<f64>,
    pub monster_ailment_threshold_table: Vec<f64>,
    pub monster_ally_damage_table: Vec<f64>,
    pub monster_ally_life_table: Vec<f64>,
    pub monster_armour_table: Vec<f64>,
    pub monster_damage_table: Vec<f64>,
    pub monster_evasion_table: Vec<f64>,
    pub monster_life_table: Vec<f64>,
    pub monster_life_table2: Vec<f64>,
    pub monster_life_table3: Vec<f64>,
    pub monster_phys_conversion_multi_table: Vec<f64>,
    pub monster_experience_level_map: Vec<f64>,
    pub map_level_boss_ailment_mult: Vec<f64>,
    pub map_level_boss_life_mult: Vec<f64>,
    /// Sparse Lua tables keyed by level (`tostring` keys, D3).
    pub map_level_life_mult: OrderedMap<f64>,
    pub monster_variety_life_mult: OrderedMap<f64>,
    pub totem_life_mult: OrderedMap<f64>,
    pub gold_respec_prices: Vec<f64>,
}

/// `table[level]` as Lua reads it (1-based); out of range is Lua `nil`.
pub fn monster_table_at(table: &[f64], level: f64) -> Option<f64> {
    if level >= 1.0 && level.fract() == 0.0 && level <= table.len() as f64 {
        Some(table[level as usize - 1])
    } else {
        None
    }
}

pub fn constants() -> &'static Constants {
    static C: OnceLock<Constants> = OnceLock::new();
    C.get_or_init(|| crate::load("constants.json"))
}
