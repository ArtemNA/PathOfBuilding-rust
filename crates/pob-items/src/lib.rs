//! `Item` (Classes/Item.lua: ParseRaw, BuildModList, BuildModListForSlotNum, FindModifierSubstring) and
//! `itemLib.applyRange` (Modules/ItemTools.lua) -- R3-ARCH §1. Executable spec: pob-ts `src/items/*`; truth: the Lua and the
//! LuaJIT fixtures of R3.1 (`crates/pob-calc/tests/fixtures/*.luajit.json`, keys `items`/`applyRange`).
//!
//! Mods come from `pob_parser::parse_mod_line` (ModCache only until R5): a cache miss gives `nil, line` exactly like the
//! oracle's parser on a line it cannot read -- the `item-miss` class of the R3 gate.

#![forbid(unsafe_code)]

pub mod apply_range;
pub mod build_raw;
pub mod item;
pub mod lua;
pub mod mod_tools;

pub use apply_range::{apply_range, apply_value_scalar, format_value, is_zero_value_line};
pub use item::{
    Affix, AffixRange, BaseLine, DataTable, GrantedSkill, INFLUENCE_INFO, Item, ItemOptions, LINE_FLAGS, ModLine, NumKey, Requirements,
    SlotMod, Socket,
};
pub use mod_tools::{format_mod, format_source_mod, set_source};
