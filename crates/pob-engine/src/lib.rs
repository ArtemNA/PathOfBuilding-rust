//! `pob-engine`: the public engine API the UI will use (R8, `KNOWLEDGE/R8-ARCH.md`): the contract is
//! `project_15/pob-engine-wasm/src/lua/boot.lua` (`engine_*`). The R8.1 LuaJIT fixtures live in `tests/fixtures/r8/`.
//!
//! - [`display`] (R8.3): the sidebar stats -- Build.lua `FormatStat` / `AddDisplayStatList` / `InsertItemWarnings` / `RefreshStatList`
//!   over [`stats`] (`Modules/BuildDisplayStats.lua`), and the structure of `engine_displayStatsJSON`; gate `tests/display.rs`.

#![forbid(unsafe_code)]

pub mod display;
pub mod lua;
pub mod stats;
pub mod view;
