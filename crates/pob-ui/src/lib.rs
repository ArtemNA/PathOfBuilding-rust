//! `pob-ui` — line-by-line port of PoB's Lua UI classes onto the `pob-gfx` host (KNOWLEDGE/UI-ARCH.md §4.2).
//!
//! Class model (U2a.0 spike): `Rc<dyn Control>` + per-field interior mutability — see [`control`].
//! Port map: `Classes/Control.lua` → [`control`], `ControlHost.lua` → [`host`], `Tooltip.lua` + `TooltipHost.lua` →
//! [`tooltip`], `<X>Control.lua` → [`controls`]; Common.lua/Main.lua helpers → [`common`]; host calls → [`sg`].

// Ported classes keep Lua's callback fields (`RefCell<Option<Rc<dyn Fn…>>>`) and argument lists.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod common;
pub mod control;
pub mod controls;
pub mod host;
pub mod lua;
pub mod search;
pub mod sg;
pub mod tooltip;
pub mod undo;

pub use control::{Anchor, Control, ControlBase, ControlRef, Prop, PropCell, Rect, anchor, rect};
pub use host::{ControlHost, InputEvent, InputEvents};
pub use tooltip::{Tooltip, TooltipHost};
