//! `pob-ui` — line-by-line port of PoB's Lua UI classes onto the `pob-gfx` host (KNOWLEDGE/UI-ARCH.md §4.2).
//!
//! Class model (U2a.0 spike): `Rc<dyn Control>` + per-field interior mutability — see [`control`].
//! Port map: `Classes/Control.lua` → [`control`], `ControlHost.lua` → [`host`], `Tooltip.lua` + `TooltipHost.lua` →
//! [`tooltip`], `<X>Control.lua` → [`controls`]; Common.lua/Main.lua helpers → [`common`]; host calls → [`sg`].

// Ported classes keep Lua's callback fields (`RefCell<Option<Rc<dyn Fn…>>>`) and argument lists.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod build_sites;
pub mod colors;
pub mod common;
pub mod control;
pub mod controls;
pub mod gem_tooltip;
pub mod host;
pub mod item_tooltip;
pub mod launch;
pub mod lua;
pub mod lua_data;
pub mod modules;
pub mod poe_api;
pub mod rate_limiter;
pub mod search;
pub mod sg;
pub mod stat_describer;
pub mod tabs;
pub mod tooltip;
pub mod tree_art;
pub mod tree_tooltip;
pub mod tree_view;
pub mod undo;

pub use control::{Anchor, Control, ControlBase, ControlRef, Prop, PropCell, Rect, anchor, rect};
pub use host::{ControlHost, InputEvent, InputEvents};
pub use tooltip::{Tooltip, TooltipHost};
