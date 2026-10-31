//! The build file: XML with `runtime/lua/xml.lua` semantics (D22) and `BuildModel` -- every attribute the tab `Load`s read,
//! with Lua-faithful types (CALC-ARCH §2). Executable spec: pob-ts `src/build/{xml,model,load}.ts`. No gem/item/tree
//! resolution here (R3.3-R3.5), no global state (W12): every load starts from the passed-in text alone.

#![forbid(unsafe_code)]

pub mod config;
pub mod load;
pub mod model;
pub mod sanitise;
pub mod xml;

pub use config::{ConfigModLists, ConfigState, ConfigValue};
pub use load::{active_config, active_item_set, active_skill_groups, active_spec, load_build_xml, meta_of};
pub use model::*;
pub use sanitise::sanitise_text;
pub use xml::{XmlElement, XmlError, XmlNode, parse_xml};
