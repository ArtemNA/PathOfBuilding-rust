//! LuaJIT runtime semantics PoB depends on, bit for bit (`KNOWLEDGE/RUST-ARCH.md` §3, MINEFIELD L1-L7, L13, L16).
//! Truth for this crate is desktop LuaJIT (docker), not the wasm oracle (LESSONS 18, O9).

#![forbid(unsafe_code)]

pub mod format;
pub mod number;
pub mod string;
pub mod tonumber;

pub use format::{Arg, format, format_f, format_g, tostring, tostring_num};
pub use tonumber::tonumber;
