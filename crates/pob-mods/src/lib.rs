//! The PoB mod core (`reference/pob-ts/KNOWLEDGE/MOD-CORE.md`): `Mod`/`Tag`, the two stores (`ModDB`, `ModList`) over the
//! shared `ModStore` base, `EvalMod` with its 21 tag branches, and `formatTag`/`compareModParams`.
//! Ported line by line from `Classes/{ModStore,ModDB,ModList}.lua` and `Modules/ModTools.lua` (upstream sha `ed354c2f8`);
//! the truth is desktop LuaJIT, checked against the corpora in `tests/fixtures/modstore-*.json`.
//! Every Lua error the core can raise (MINEFIELD L18) comes back as a `LuaError` carrying LuaJIT's message text.

#![forbid(unsafe_code)]

pub mod calc_lib;
pub mod data;
mod eval;
pub mod matching;
pub mod store;
pub mod types;
pub mod value;

pub use matching::{compare_mod_params, format_tag, format_tags, match_keyword_flags};
pub use store::{GlobalLimits, ModDb, ModList, ModStore, StoreKind, StoreRef, TabEntry};
pub use types::{ActiveSkill, Actor, ActorLink, ActorRef, Gem, GrantedEffect, ItemLike, Mod, ModCfg, ModType, Tag, TagKind, create_mod};
pub use value::{LuaError, LuaResult, Str, Table, Value};
