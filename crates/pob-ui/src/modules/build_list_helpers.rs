//! `Modules/BuildListHelpers.lua`: scanning, filtering and sorting the builds folder (`main.buildPath`).

use crate::controls::ByRef;
use crate::lua;
use std::cmp::Ordering;
use std::rc::Rc;

/// A build-list entry (a Lua table, compared by identity): a build file or a folder.
#[derive(Clone, Debug, Default)]
pub struct BuildEntry {
    pub file_name: Option<String>,
    pub folder_name: Option<String>,
    pub sub_path: String,
    pub full_file_name: String,
    pub modified: f64,
    pub build_name: Option<String>,
    pub level: Option<f64>,
    pub class_name: Option<String>,
    pub ascend_class_name: Option<String>,
}

pub type BuildRef = ByRef<BuildEntry>;

/// BuildListHelpers.lua:10-15
pub const BUILD_SORT_DROP_LIST: &[(&str, &str)] = &[
    ("Sort by Name", "NAME"),
    ("Sort by Class", "CLASS"),
    ("Sort by Last Edited", "EDITED"),
    ("Sort by Level", "LEVEL"),
];

/// `io.open(name, "r"):read(n)` on Windows: text mode (`\r\n` → `\n`, Ctrl-Z ends the file), `n` characters.
pub fn read_text_mode(name: &str, n: Option<usize>) -> Option<Option<String>> {
    let bytes = std::fs::read(name).ok()?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() && n.is_none_or(|n| out.len() < n) {
        let c = bytes[i];
        if c == 0x1A {
            break;
        }
        if c == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    // `read(n)` at end of file → nil.
    if out.is_empty() && n != Some(0) {
        return Some(None);
    }
    Some(Some(String::from_utf8_lossy(&out).into_owned()))
}

pub struct Header {
    pub level: Option<f64>,
    pub class_name: Option<String>,
    pub ascend_class_name: Option<String>,
}

/// `ReadBuildHeader(fullFileName)` (BuildListHelpers.lua:17-34): `None` = the cloud error popup was opened.
fn read_build_header(g: &mut pob_gfx::Gfx, full_file_name: &str) -> Option<Header> {
    let empty = Header {
        level: None,
        class_name: None,
        ascend_class_name: None,
    };
    let Some(text) = read_text_mode(full_file_name, Some(2048)) else {
        return Some(empty);
    };
    let Some(header_text) = text else {
        super::main::main().open_cloud_error_popup(g, Some(full_file_name));
        return None;
    };
    let Some(tag) = lua::lmatch(&header_text, "<Build%s+[^>]->") else {
        return Some(empty);
    };
    Some(Header {
        level: lua::lmatch(&tag, "level=\"([^\"]+)\"").and_then(|s| pob_lua::tonumber(&s)),
        class_name: lua::lmatch(&tag, "className=\"([^\"]+)\""),
        ascend_class_name: lua::lmatch(&tag, "ascendClassName=\"([^\"]+)\""),
    })
}

fn find_plain(hay: &str, needle: &str) -> bool {
    hay.contains(needle)
}

/// `MatchEntry(entry, terms)` (BuildListHelpers.lua:36-56).
fn match_entry(e: &BuildEntry, terms: &[String]) -> bool {
    let low = |s: &Option<String>| s.as_ref().map(|s| s.to_ascii_lowercase());
    for term in terms {
        let term_lower = term.to_ascii_lowercase();
        if let Some(val) = lua::lmatch(&term_lower, "^class:(.*)$") {
            if !val.is_empty() {
                let m = low(&e.class_name).is_some_and(|c| find_plain(&c, &val))
                    || low(&e.ascend_class_name).is_some_and(|c| find_plain(&c, &val))
                    || low(&e.folder_name).is_some_and(|c| find_plain(&c, &val));
                if !m {
                    return false;
                }
            }
        } else {
            let m = low(&e.build_name).is_some_and(|c| find_plain(&c, &term_lower))
                || low(&e.folder_name).is_some_and(|c| find_plain(&c, &term_lower))
                || low(&e.class_name).is_some_and(|c| find_plain(&c, &term_lower))
                || low(&e.ascend_class_name).is_some_and(|c| find_plain(&c, &term_lower))
                || Some(e.sub_path.to_ascii_lowercase()).is_some_and(|c| find_plain(&c, &term_lower));
            if !m {
                return false;
            }
        }
    }
    true
}

/// `ScanFolder(subPath)` (BuildListHelpers.lua:59-105).
pub fn scan_folder(g: &mut pob_gfx::Gfx, build_path: &str, sub_path: &str) -> Vec<BuildRef> {
    let mut list = Vec::new();
    scan_dir(g, build_path, sub_path, &mut list);
    list
}

fn scan_dir(g: &mut pob_gfx::Gfx, build_path: &str, current: &str, list: &mut Vec<BuildRef>) {
    let mut handle = pob_gfx::fs::new_file_search(&format!("{build_path}{current}*.xml"), false);
    while let Some(h) = &mut handle {
        let file_name = h.get_file_name().to_string();
        let full_file_name = format!("{build_path}{current}{file_name}");
        if let Some(header) = read_build_header(g, &full_file_name) {
            list.push(ByRef(Rc::new(BuildEntry {
                build_name: Some(lua::gsub(&file_name, "%.xml$", "")),
                file_name: Some(file_name),
                sub_path: current.to_string(),
                full_file_name,
                modified: h.get_file_modified_time(),
                level: header.level,
                class_name: header.class_name,
                ascend_class_name: header.ascend_class_name,
                folder_name: None,
            })));
        }
        if !h.next_file() {
            break;
        }
    }
    let mut handle = pob_gfx::fs::new_file_search(&format!("{build_path}{current}*"), true);
    let mut sub_folders = Vec::new();
    while let Some(h) = &mut handle {
        sub_folders.push((h.get_file_name().to_string(), h.get_file_modified_time()));
        if !h.next_file() {
            break;
        }
    }
    for (folder_name, modified) in sub_folders {
        let next = format!("{current}{folder_name}/");
        list.push(ByRef(Rc::new(BuildEntry {
            full_file_name: format!("{build_path}{current}{folder_name}"),
            folder_name: Some(folder_name),
            sub_path: current.to_string(),
            modified,
            ..Default::default()
        })));
        scan_dir(g, build_path, &next, list);
    }
}

/// `FilterList(index, subPath, filterText)` (BuildListHelpers.lua:108-122).
pub fn filter_list(index: &[BuildRef], sub_path: &str, filter_text: Option<&str>) -> Vec<BuildRef> {
    let terms: Vec<String> = filter_text
        .unwrap_or("")
        .split(|c: char| c.is_ascii_whitespace() || c == '\x0b')
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    index
        .iter()
        .filter(|e| (terms.is_empty() && e.0.sub_path == sub_path) || (!terms.is_empty() && match_entry(&e.0, &terms)))
        .cloned()
        .collect()
}

/// `CanMoveToSubPath(build, targetSubPath)` (BuildListHelpers.lua:124-132).
pub fn can_move_to_sub_path(build: &BuildEntry, target: &str) -> bool {
    if build.sub_path == target {
        return false;
    }
    if let Some(folder) = &build.folder_name {
        let source = format!("{}{folder}/", build.sub_path);
        if target.as_bytes().starts_with(source.as_bytes()) {
            return false;
        }
    }
    true
}

/// `naturalSortCompare(a, b)` (Common.lua:673-704): `a < b`.
pub fn natural_sort_compare(a: &str, b: &str) -> bool {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut ai, mut bi) = (1i64, 1i64);
    let m = |s: &[u8], i: i64| -> Option<(Vec<u8>, Vec<u8>, i64)> {
        let caps = pob_lua::match_(s, b"(.-)(%d+)()", Some(i)).expect("pattern")?;
        let get = |k: usize| match caps.get(k) {
            Some(pob_lua::Capture::Str { start, end }) => s[start..end].to_vec(),
            _ => Vec::new(),
        };
        let end = match caps.get(2) {
            Some(pob_lua::Capture::Pos(p)) => p as i64,
            _ => unreachable!(),
        };
        Some((get(0), get(1), end))
    };
    let sub_from = |s: &[u8], i: i64| -> Vec<u8> { s.get((i - 1).max(0) as usize..).map(<[u8]>::to_vec).unwrap_or_default() };
    loop {
        let am = m(ab, ai);
        let bm = m(bb, bi);
        let (mut a_str, mut b_str) = (am.as_ref().map(|x| x.0.clone()), bm.as_ref().map(|x| x.0.clone()));
        let (a_num, b_num) = (am.as_ref().map(|x| x.1.clone()), bm.as_ref().map(|x| x.1.clone()));
        if a_str.is_none() || b_str.is_none() {
            a_str = Some(sub_from(ab, ai));
            b_str = Some(sub_from(bb, bi));
        }
        let (a_str, b_str) = (a_str.expect("a"), b_str.expect("b"));
        let al = a_str.to_ascii_uppercase();
        let bl = b_str.to_ascii_uppercase();
        if al != bl {
            return al < bl;
        }
        if a_str != b_str {
            return a_str < b_str;
        }
        let Some(a_num) = a_num else { return ab < bb };
        let b_num = b_num.expect("bNum");
        let an = pob_lua::tonumber(&String::from_utf8_lossy(&a_num)).expect("number");
        let bn = pob_lua::tonumber(&String::from_utf8_lossy(&b_num)).expect("number");
        if an != bn {
            return an < bn;
        }
        if a_num != b_num {
            return a_num < b_num;
        }
        ai = am.expect("a").2;
        bi = bm.expect("b").2;
    }
}

/// A Lua `table.sort` comparator (`a < b`) as an `Ordering` (the comparators here are total orders, so the result
/// does not depend on the sort algorithm — MINEFIELD L17).
fn ord(less: impl Fn(&BuildEntry, &BuildEntry) -> bool) -> impl Fn(&BuildRef, &BuildRef) -> Ordering {
    move |a, b| {
        if less(&a.0, &b.0) {
            Ordering::Less
        } else if less(&b.0, &a.0) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

/// `SortList(list, sortMode)` (BuildListHelpers.lua:136-180).
pub fn sort_list(list: &mut [BuildRef], sort_mode: &str) {
    let mode = sort_mode.to_string();
    list.sort_by(ord(move |a, b| {
        let a_folder = a.folder_name.is_some();
        let b_folder = b.folder_name.is_some();
        if a_folder && !b_folder {
            return true;
        }
        if !a_folder && b_folder {
            return false;
        }
        let name = |e: &BuildEntry| {
            format!(
                "{}{}",
                e.sub_path,
                e.folder_name.as_deref().or(e.file_name.as_deref()).unwrap_or("")
            )
        };
        let (ap, bp) = (name(a), name(b));
        if mode == "EDITED" {
            if a.modified != b.modified {
                return a.modified > b.modified;
            }
            return natural_sort_compare(&ap, &bp);
        }
        if a_folder {
            return natural_sort_compare(&ap, &bp);
        }
        if mode == "CLASS" {
            match (&a.class_name, &b.class_name) {
                (None, Some(_)) => return true,
                (Some(_), None) => return false,
                (Some(x), Some(y)) if x != y => return x < y,
                _ => {}
            }
            match (&a.ascend_class_name, &b.ascend_class_name) {
                (None, Some(_)) => return true,
                (Some(_), None) => return false,
                (Some(x), Some(y)) if x != y => return x < y,
                _ => {}
            }
            natural_sort_compare(&ap, &bp)
        } else if mode == "LEVEL" {
            match (a.level, b.level) {
                (Some(_), None) => return false,
                (None, Some(_)) => return true,
                (Some(x), Some(y)) if x != y => return x < y,
                _ => {}
            }
            natural_sort_compare(&ap, &bp)
        } else {
            natural_sort_compare(&ap, &bp)
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::natural_sort_compare as lt;

    #[test]
    fn natural_sort() {
        assert!(lt("a2", "a10"));
        assert!(!lt("a10", "a2"));
        assert!(lt("Abc", "abd"));
        assert!(lt("ABC", "abc"));
        assert!(lt("a01", "a1"));
        assert!(lt("x", "x1"));
        assert!(!lt("same", "same"));
    }
}
