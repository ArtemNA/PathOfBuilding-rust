//! `pob-gfx` — own implementation of the SimpleGraphic host API that PoB's Lua UI is written against
//! (KNOWLEDGE/UI-ARCH.md §4.1). Semantics follow the original `SimpleGraphic v2.5-3b1a346` as measured by the
//! SG oracle (`tools/sg-oracle`), not its C++ sources (not available locally).
//!
//! - [`text`] (U1.1): GPU-free text functions — `.tgf`/`.tga` fonts, `DrawStringWidth`, `DrawStringCursorIndex`,
//!   `StripEscapes`.
//! - [`draw`] (U1.2): host state [`Gfx`], the per-frame API [`Frame`] (`SetDrawLayer`, `SetViewport`, `SetDrawColor`,
//!   `DrawImage`, `DrawImageQuad`, `DrawString`) and the frame's [`DrawList`].
//! - [`image`]: `NewImageHandle` / `Load` (incl. `ASYNC`) / `ImageSize`.
//! - [`render`]: the `wgpu` renderer (window surface or offscreen RGBA readback).
//! - [`input`] (U1.4): key names, `OnKeyDown`/`OnKeyUp`/`OnChar` translation, `IsKeyDown`, `GetCursorPos`.
//! - [`platform`]: clock (`GetTime`), clipboard (`Copy`/`Paste`), `OpenURL`; a scripted [`platform::TestPlatform`].
//! - [`fs`] (U2b): `NewFileSearch`, `MakeDir`, `RemoveDir`, `GetUserPath`, `GetCloudProvider`.
//! - [`screenshot`]: `TakeScreenshot` files.
//! - [`window`]: the `winit` loop calling a [`MainObject`]'s callbacks.

pub mod color;
pub mod draw;
pub mod fs;
pub mod image;
pub mod input;
pub mod platform;
pub mod render;
pub mod screenshot;
pub mod text;
pub mod trace;
pub mod window;

pub use draw::{Align, Command, DrawList, Frame, Gfx, GfxError, WindowRequests};
pub use image::{ImageHandle, LoadFlag, Texture};
pub use input::{Button, Callback, KeyId, PhysKey, RawEvent};
pub use platform::{Platform, SystemPlatform, TestPlatform};
pub use render::Renderer;
pub use window::{MainObject, WindowConfig, run};
