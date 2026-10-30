# pob-rust

A personal port of **Path of Building** (Path of Exile build planner, MIT, (c) Path of Building Community) from Lua to **Rust**: the calculation core
and the desktop interface. Not a fork for publication; the goal is speed for one user.

## Mandates

- **R-M1. Number parity.** Every `getStats()` key is bit for bit with desktop PoB (LuaJIT); checked by goldens, the wasm oracle and LuaJIT in docker.
  Quirks and typos of upstream are reproduced literally; anything skipped is an explicit `unimplemented`.
- **R-M2. Data is not moved by hand.** The source is the pob-ts converter (`data/*.json` with `manifest.json`/pobSha); closures in the data are registries with a sha.
- **R-M3. The interface as in the original.** Our own implementation of the SimpleGraphic API (`winit` + `wgpu`, the same bitmap fonts and assets) and a port of the Lua UI classes.
- **R-M4. Speed comes after correctness.** Optimizations only for modules in parity, with a before/after benchmark; mass recalculations run in parallel.
- **R-M5. Process.** A step-by-step plan with each step approved, task cards for subagents, reviews that rerun the checks, a local git (`ORCHESTRATION.md`).

## Where things are

- `PROMPT.md`: the full starting prompt (goal, sources, process, architecture, phases, pitfalls, environment).
- `PLAN.md`: phases R0-R8 (core) and U0-U9 (UI) with their status.
- `KNOWLEDGE/`: new pitfalls and lessons of this repo, `RUST-ARCH.md` (from R0).
- `reference/`: references: `INDEX.md`, `POB-TS-MAP.md`, `UI-SURFACE.md`, copies of the pob-ts documents (`reference/pob-ts/`).
- `tasks/`: task cards for subagents.

## Attribution

Path of Building is (c) Path of Building Community, MIT license (`PathOfBuilding/LICENSE.md`); game data is (c) Grinding Gear Games.
Fonts and assets are used from a local checkout of the original and are not copied into the git of this repo.
