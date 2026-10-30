//! `data/config-options.json` -- the structure of Modules/ConfigOptions.lua (CALC-ARCH §6), pob-ts `src/data/configOptions.ts`.
//! `apply`/`tooltipFunc` are closure markers (`$fn` + sha of the body): the bodies are ported by `var` in pob-build's
//! registry (R3.5, R3-ARCH §2.5), never read from here.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::{OrderedMap, Scalar};

/// A Lua closure in the data: where it is and the sha256 of its body (drift test of the Rust registry).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureRef {
    pub src: String,
    pub from: f64,
    pub to: f64,
    pub sha: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigListEntry {
    pub val: Scalar,
    pub label: String,
}

/// An upstream authoring bug kept verbatim (2 vars); ConfigTab.lua never reads it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Malformed {
    pub field: String,
    pub value: Json,
}

/// One `{var=,type=,...}` field, or a var-less `{label=,ifSkill=,...}` sub-header (93 of these).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigVar {
    pub var: Option<String>,
    /// `check`/`count`/`countAllowZero`/`integer`/`float`/`list`/`text`.
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub label: Option<String>,
    pub section: String,
    pub index: f64,
    pub list: Option<Vec<ConfigListEntry>>,
    pub default_state: Option<Scalar>,
    pub default_index: Option<f64>,
    pub default_placeholder_state: Option<Scalar>,
    /// A string, or (2 vars) a closure marker.
    pub tooltip: Option<Json>,
    pub tooltip_func: Option<ClosureRef>,
    pub apply: Option<ClosureRef>,
    /// `ifCond`, `ifSkill`, `implyCond`, ... verbatim.
    pub gates: OrderedMap<Json>,
    pub malformed: Option<Vec<Malformed>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSection {
    pub section: String,
    pub col: f64,
    pub index: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigOptions {
    pub format: f64,
    pub sections: Vec<ConfigSection>,
    /// In ConfigOptions.lua order (the order `NewConfigSet` seeds defaults in).
    pub vars: Vec<ConfigVar>,
}

impl ConfigOptions {
    /// The var with this name (first one -- names are unique in the data).
    pub fn var(&self, name: &str) -> Option<&ConfigVar> {
        self.vars.iter().find(|v| v.var.as_deref() == Some(name))
    }
}

pub fn config_options() -> &'static ConfigOptions {
    static C: OnceLock<ConfigOptions> = OnceLock::new();
    C.get_or_init(|| crate::load("config-options.json"))
}
