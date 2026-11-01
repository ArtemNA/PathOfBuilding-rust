# builds/: the corpus of builds for parity checks

The builds on which every upstream update (R-UP and future releases per `UPDATE-PROMPT.md`) compares Rust with LuaJIT.
The goal is that the new data and mechanics of each release are compared too.

## Adding a build

- Put a file in `builds/user/`: a PoB XML (`*.xml`, "Export > Copy XML" or a saved file) or a build code (`*.txt`, one line, a code from pobb.in or "Generate").
- Name it for what is interesting in it: `minion-srs-spectres.xml`, `trigger-cospris.txt`.
- Optionally: the first line of a `.txt` or a comment `<!-- note: ... -->` in an XML says which mechanic the build is for (the agent reads it).

## Layout

- `user/`: the user's builds (added by hand; the agent neither deletes nor changes them).
- `pc/`: a copy of the user's `Documents\Path of Building\Builds` (138 XML, leagues 3.24-3.29; **a copy only**: leave the originals alone, update only by copying from there; not in git (`.gitignore`), because they are personal builds and the repo is pushed to GitHub).
- `corpus/`: builds collected by the orchestrator or an agent: goldens, synthetic R4/R7/R8 (filled in R-UP).

**Priority (2026-11-02, the user's wish):** the checks are based on builds of the **current league** (now `pc/3.29`, 20 builds; upstream PoB 2.67.2 has tree 3_29); of the older leagues,
a few per league (tree conversion path, changed mechanics). After R-UP, "current" is the newest league in `pc/`.

Update agent: every build from here -> LuaJIT reference -> Rust = LuaJIT (output, breakdown, display); a build that failed in LuaJIT
on a new version (removed gems or items) goes into the report and is not deleted.
