//! ModTools.lua:136-245 (`formatFlags`, `formatValue`, `formatModParams`, `formatMod`, `formatSourceMod`, `setSource`):
//! moved to [`pob_mods::format`] (R3.8b); re-exported here so existing `pob_tree::modtools::*` paths keep working.

pub use pob_mods::format::{format_flags, format_mod, format_mod_params, format_source_mod, format_value, set_source};
