//! The build file: XML with `runtime/lua/xml.lua` semantics (verbatim, R3.8b; D22) and `BuildModel` -- every attribute the
//! tab `Load`s read, with Lua-faithful types (CALC-ARCH §2) -- plus the ConfigTab state (`config`). Truth: the Lua and the
//! R3.8a LuaJIT corpora (`tests/fixtures/{xml-lua,config-load}.json`). No gem/item/tree resolution here, no global state
//! (W12): every load starts from the passed-in text alone.

#![forbid(unsafe_code)]

pub mod config;
pub mod file;
pub mod load;
pub mod model;
pub mod sanitise;
pub mod xml;

pub use config::{ConfigModLists, ConfigState, ConfigValue};
pub use file::{desktop_text_mode, read_build_file};
pub use load::{
    BuildLoadError, LoadedBuild, active_config, active_item_set, active_skill_groups, active_spec, load_build, load_build_xml, meta_of,
};
pub use model::*;
pub use sanitise::sanitise_text;
pub use xml::{RawXmlElement, RawXmlNode, XmlElement, XmlError, XmlNode, parse_xml_bytes, parse_xml_document};
