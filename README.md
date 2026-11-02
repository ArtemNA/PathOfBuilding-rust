# pob-rust

A personal port of **Path of Building** (Path of Exile build planner, MIT, (c) Path of Building Community) from Lua to **Rust**: the calculation core
and the desktop interface. Not a fork for publication; the goal is speed for one user.

## Mandates

- **R-M1. Number parity.** Every `getStats()` key is bit for bit with desktop PoB (LuaJIT); checked by goldens, the wasm oracle and LuaJIT in docker.
  Quirks and typos of upstream are reproduced literally; anything skipped is an explicit `unimplemented`.
- **R-M2. Data is not moved by hand.** The source is the pob-ts converter (`data/*.json` with `manifest.json`/pobSha); closures in the data are registries with a sha.
- **R-M3. The interface as in the original.** Our own implementation of the SimpleGraphic API (`winit` + `wgpu`, the same bitmap fonts and assets) and a port of the Lua UI classes.
- **R-M4. Speed comes after correctness.** Optimizations only for modules in parity, with a before/after benchmark; mass recalculations run in parallel.

## Attribution

Path of Building is (c) Path of Building Community, MIT license (`PathOfBuilding/LICENSE.md`); game data is (c) Grinding Gear Games.
Fonts and assets are used from a local checkout of the original and are not copied into the git of this repo.
