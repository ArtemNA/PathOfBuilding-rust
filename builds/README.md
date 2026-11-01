# builds/: the corpus of builds for parity checks

The builds on which every upstream update (R-UP and future releases per `UPDATE-PROMPT.md`) compares Rust with LuaJIT.
The goal is that the new data and mechanics of each release are compared too.

## Adding a build

- Put a file in `builds/user/`: a PoB XML (`*.xml`, "Export > Copy XML" or a saved file) or a build code (`*.txt`, one line, a code from pobb.in or "Generate").
- Name it for what is interesting in it: `minion-srs-spectres.xml`, `trigger-cospris.txt`.
- Optionally: the first line of a `.txt` or a comment `<!-- note: ... -->` in an XML says which mechanic the build is for (the agent reads it).

## Layout

- `user/`: the user's builds (added by hand; the agent neither deletes nor changes them).
- `corpus/`: builds collected by the orchestrator or an agent: goldens, synthetic R4/R7/R8 (filled in R-UP).

Update agent: every build from here -> LuaJIT reference -> Rust = LuaJIT (output, breakdown, display); a build that failed in LuaJIT
on a new version (removed gems or items) goes into the report and is not deleted.
