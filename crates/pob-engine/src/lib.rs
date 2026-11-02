//! `pob-engine`: the public engine API the UI will use (R8, `KNOWLEDGE/R8-ARCH.md`): the contract is
//! `project_15/pob-engine-wasm/src/lua/boot.lua` (`engine_*`). The R8.1 LuaJIT fixtures live in `tests/fixtures/r8/`.
//!
//! - [`calcs`] (R8.6): the Calcs tab -- `Modules/CalcSections.lua` (+ the `SkillSelect` section), `CheckFlag` / `UpdateSize` visibility and the
//!   text of every string `CalcSectionControl:DrawContent` draws (`formatCalcStr`), over the CALCS run (`CalcSession::build_output(Calcs)`);
//!   gate `tests/calcs.rs`.
//! - [`engine`] (R8.8): [`engine::Engine`] -- `engine_loadBuildXML` / `recalc` / the mutators (`allocNode`, `deallocNode`, `setConfig`,
//!   `setGem`, `setMainSocketGroup`) / the readers ([`api`]: `outputJSON`, `statsForJSON`, `fullDPSJSON`, `skillListJSON`, `configListJSON`,
//!   `displayStatsJSON`, the power report) over a model-derived run (no hidden state); gates `tests/engine.rs` (path equivalence),
//!   `tests/engine_luajit.rs` (= LuaJIT, fixtures of `tools/lua-corpus/export-engine.ts`).
//! - [`save`] (R8.8): `Build:SaveDB` -- every tab's `Save`; gate `tests/save.rs` (= the R8.1 LuaJIT `SaveDB`, `load(save(b)) == b`).
//! - [`display`] (R8.3): the sidebar stats -- Build.lua `FormatStat` / `AddDisplayStatList` / `InsertItemWarnings` / `RefreshStatList`
//!   over [`stats`] (`Modules/BuildDisplayStats.lua`), and the structure of `engine_displayStatsJSON`; gate `tests/display.rs`.
//! - [`shell`] (U2c): what the Build screen's shell reads and edits -- `RefreshSkillSelectControls`, the point display, the class /
//!   ascendancy drop-downs, the main-skill / level mutators; gate `tests/shell.rs` (path equivalence).

#![forbid(unsafe_code)]

pub mod api;
pub mod calcs;
pub mod display;
pub mod engine;
pub mod lua;
pub mod save;
pub mod shell;
pub mod stats;
pub mod view;
