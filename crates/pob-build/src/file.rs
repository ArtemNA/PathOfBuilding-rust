//! Reading a build file the way desktop PoB does: `buildMode:LoadDBFile` opens it with `io.open(self.dbFileName, "r")`
//! (Modules/Build.lua:2267) -- on Windows that is the MSVC CRT text mode, which turns every `\r\n` into `\n` while reading.
//! `xml.lua` itself never touches line ends (`crate::xml` stays verbatim), so this is the only place that does.
//!
//! Two input paths, kept apart on purpose:
//! - "as desktop": [`read_build_file`] / [`desktop_text_mode`] -- `pob-cli` (file and stdin) and tests that model the
//!   desktop load (e.g. the `BuildModel` comparison against pob-ts, which normalised CRLF too);
//! - "as the docker fixture": the raw bytes, unnormalised -- the LuaJIT fixtures of R3.1/R3.8a were taken in a Linux
//!   container where `"r"` keeps `\r`, so tests comparing against them feed the same bytes the probe saw.
//!
//! Not emulated: MSVC text mode also stops at a Ctrl-Z (0x1A) byte as end of file; no build file contains one.

use std::path::Path;

/// MSVC CRT text-mode read: `\r\n` -> `\n`; a lone `\r` is kept.
pub fn desktop_text_mode(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// `io.open(path, "r"):read("*a")` on desktop Windows (Build.lua:2267-2272), as UTF-8 text.
pub fn read_build_file(path: impl AsRef<Path>) -> std::io::Result<String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: not UTF-8: {e}", path.display())))?;
    Ok(desktop_text_mode(&text))
}
