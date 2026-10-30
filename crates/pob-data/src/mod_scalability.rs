//! `data.modScalability` (Data/ModScalability.lua) -- the table `itemLib.applyRange` looks a value-stripped line up in
//! (ItemTools.lua:146, :182). pob-ts `src/data/modScalability.ts`. Only lookups: a `HashMap` is fine (RUST-ARCH §4).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// One entry per `#` left in the looked-up line, in order (ItemTools.lua:182 walks them with `ipairs`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModScalabilityEntry {
    pub is_scalable: bool,
    /// GGG stat format ids (`divide_by_ten_1dp`, ...); ItemTools.lua:185-285.
    pub formats: Option<Vec<String>>,
}

pub type ModScalability = HashMap<String, Vec<ModScalabilityEntry>>;

/// `data.modScalability` (15 202 forms).
pub fn mod_scalability() -> &'static ModScalability {
    static M: OnceLock<ModScalability> = OnceLock::new();
    M.get_or_init(|| crate::load("mod-scalability.json"))
}
