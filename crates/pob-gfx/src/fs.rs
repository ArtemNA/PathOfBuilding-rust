//! File functions of the host (UI-ARCH §2.7): `NewFileSearch`, `MakeDir`, `RemoveDir`, `GetUserPath`, `GetCloudProvider`.
//! Semantics measured by the SG oracle (`tools/sg-oracle/lua/ui-runner.lua`, probe in `fixtures/ui/*.json`):
//! - `NewFileSearch(spec[, dirs])`: `FindFirstFile` order (NTFS: by upper-cased name, `std::fs::read_dir` on Windows
//!   uses the same API); wildcards `*`/`?` case-insensitive; `dirs` = directories only (no `.`/`..`), otherwise files
//!   only; no match → nothing (Lua: no return value, here `None`). `GetFileModifiedTime` = the raw `FILETIME`
//!   (100 ns ticks since 1601) as a number.
//! - `MakeDir(path)`: one level (missing parent → `nil, "No such process"` — the CRT text of Win32 error 3);
//!   existing directory → `nil, "No error"`; a file in the way → `nil, "Unknown error"`.
//! - `RemoveDir(path[, recursive])`: non-empty without `recursive` → `nil, "Unknown error"`; missing →
//!   `nil, "No such file or directory"`.
//! - `GetCloudProvider(path)`: nothing for a local folder.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// An open `NewFileSearch` handle positioned on a match.
#[derive(Clone, Debug)]
pub struct FileSearch {
    entries: Vec<(String, f64)>,
    pos: usize,
}

/// Seconds between 1601-01-01 and 1970-01-01.
const FILETIME_UNIX_DIFF: u64 = 11_644_473_600;

fn filetime(t: SystemTime) -> f64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => ((d.as_secs() + FILETIME_UNIX_DIFF) * 10_000_000 + u64::from(d.subsec_nanos() / 100)) as f64,
        Err(_) => 0.0,
    }
}

/// `*` / `?` match, ASCII case-insensitive (Win32 file name matching).
fn wild_match(pat: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while n < name.len() {
        if p < pat.len() && (pat[p] == b'?' || pat[p].eq_ignore_ascii_case(&name[n])) {
            p += 1;
            n += 1;
        } else if p < pat.len() && pat[p] == b'*' {
            star = Some(p);
            p += 1;
            mark = n;
        } else if let Some(s) = star {
            p = s + 1;
            mark += 1;
            n = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == b'*' {
        p += 1;
    }
    p == pat.len()
}

/// `NewFileSearch(spec, findDirectories)`.
pub fn new_file_search(spec: &str, find_dirs: bool) -> Option<FileSearch> {
    let cut = spec.rfind(['/', '\\']);
    let (dir, pat) = match cut {
        Some(i) => (&spec[..=i], &spec[i + 1..]),
        None => ("./", spec),
    };
    let rd = std::fs::read_dir(dir).ok()?;
    let mut entries = Vec::new();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let is_dir = ft.is_dir();
        if is_dir != find_dirs {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if !wild_match(pat.as_bytes(), name.as_bytes()) {
            continue;
        }
        let modified = e.metadata().and_then(|m| m.modified()).map(filetime).unwrap_or(0.0);
        entries.push((name, modified));
    }
    (!entries.is_empty()).then_some(FileSearch { entries, pos: 0 })
}

impl FileSearch {
    /// `handle:GetFileName()`.
    pub fn get_file_name(&self) -> &str {
        &self.entries[self.pos].0
    }

    /// `handle:GetFileModifiedTime()`.
    pub fn get_file_modified_time(&self) -> f64 {
        self.entries[self.pos].1
    }

    /// `handle:NextFile()` → `false` when there are no more matches.
    pub fn next_file(&mut self) -> bool {
        if self.pos + 1 < self.entries.len() {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

/// `MakeDir(path)` → `Ok(())` = `true`; `Err(msg)` = `nil, msg`.
pub fn make_dir(path: &str) -> Result<(), String> {
    let p = Path::new(path.trim_end_matches(['/', '\\']));
    if p.is_dir() {
        return Err("No error".into());
    }
    if p.exists() {
        return Err("Unknown error".into());
    }
    match std::fs::create_dir(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err("No such process".into()),
        Err(_) => Err("Unknown error".into()),
    }
}

/// `RemoveDir(path, recursive)`.
pub fn remove_dir(path: &str, recursive: bool) -> Result<(), String> {
    let p = Path::new(path.trim_end_matches(['/', '\\']));
    if !p.exists() {
        return Err("No such file or directory".into());
    }
    let r = if recursive {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_dir(p)
    };
    r.map_err(|_| "Unknown error".to_string())
}

/// `GetCloudProvider(path)` → `(provider, version, status)`; `None` = a local folder (as measured).
pub fn get_cloud_provider(_path: &str) -> Option<(String, String, String)> {
    None
}

/// `GetUserPath()`: the user's "Documents" folder (`%USERPROFILE%\Documents`, UI-ARCH §10 п. 2: Known Folder via
/// `windows-sys` is a U9 question), with `/` separators like the original's paths.
pub fn get_user_path() -> Option<String> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(format!("{}/Documents", home.replace('\\', "/")))
}

#[cfg(test)]
mod tests {
    use super::wild_match;

    #[test]
    fn wildcards() {
        assert!(wild_match(b"*.xml", b"a b.xml"));
        assert!(wild_match(b"*.xml", b"c.XML"));
        assert!(!wild_match(b"*.xml", b"y.txt"));
        assert!(wild_match(b"*", b"anything"));
        assert!(wild_match(b"x.xml", b"X.xml"));
        assert!(!wild_match(b"x.xml", b"xx.xml"));
    }
}
