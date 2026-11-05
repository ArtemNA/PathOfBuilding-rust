# builds/: the corpus of builds for parity checks

The builds on which every upstream update compares Rust with LuaJIT, so that the new data and mechanics of each release are compared too.

## Adding a build

- Put a file in `builds/user/`: a PoB XML (`*.xml`, a saved build or "Export > Copy XML") or a build code (`*.txt`, one line, a code from pobb.in or "Generate").
- Name it for what is interesting in it: `minion-srs-spectres.xml`, `trigger-cospris.txt`.
- Optionally say which mechanic it is for: the first line of a `.txt`, or a comment `<!-- note: ... -->` in an XML.

## Layout

- `user/`: builds added by hand; updates neither delete nor change them.
- `pc/`: a local copy of personal builds, ignored by git.
- `corpus/`: collected builds: goldens and synthetic ones.

The checks are based on builds of the current league, plus a few from each older league for tree conversion and changed mechanics.

Every build goes through: LuaJIT reference, then Rust = LuaJIT (output, breakdown, display). A build that fails in LuaJIT on a new version (removed gems or items) goes into the report and is not deleted.
