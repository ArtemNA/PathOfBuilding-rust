//! `pob-gfx` — own implementation of the SimpleGraphic host API that PoB's Lua UI is written against
//! (KNOWLEDGE/UI-ARCH.md §4.1). Semantics follow the original `SimpleGraphic v2.5-3b1a346` as measured by the
//! SG oracle (`tools/sg-oracle`), not its C++ sources (not available locally).
//!
//! U1.1 provides only the GPU-free text module: `.tgf`/`.tga` font loading, height -> atlas selection,
//! `DrawStringWidth`, `DrawStringCursorIndex`, `StripEscapes`.

pub mod text;
