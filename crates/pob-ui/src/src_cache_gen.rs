//! Making `src/` of a package from upstream's (`tools/package.ps1` through `examples/gen_src_cache.rs`): the tree's files, one per content, and `src.cache`; and the check of the result against
//! upstream. Runs on the build machine only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use pob_data::bincache::write_tables;

use crate::lua_data::{LuaKey, LuaTab, LuaVal, parse_return};
use crate::src_cache::{COPIES, FILE, MAGIC, Package, encode, same};
use crate::tabs::item_popups::{POPUP_TABLES, json_to_lua, popup_key, popup_table};

type Maker = fn(&str) -> Result<LuaVal, String>;

/// What the scanners of raw text get out of it.
fn products() -> [(&'static str, Maker); 4] {
    [
        (crate::gem_tooltip::FLAVOUR_KEY, crate::gem_tooltip::make_flavour),
        (crate::item_tooltip::FLAVOUR_KEY, crate::item_tooltip::make_flavour),
        (crate::tabs::tree::VERSION_URLS_KEY, crate::tabs::tree::make_version_urls),
        (crate::tabs::tree::PLANNER_KEY, crate::tabs::tree::make_planner_versions),
    ]
}

/// The files read as whole tables, besides the trees and `Data/StatDescriptions/`.
const DATA_TABLES: [&str; 5] = [
    "Data/PearlSupports.lua",
    "Data/QueryMods.lua",
    "Data/TradeSiteStats.lua",
    "Data/CurrencyNames.lua",
    "Data/BeastCraft.lua",
];

fn with_fields(t: &LuaTab, keep: impl Fn(&str, &LuaVal) -> Option<LuaVal>) -> LuaVal {
    let fields = t
        .fields
        .iter()
        .filter_map(|(k, v)| match k {
            LuaKey::Str(s) => Some((k.clone(), keep(s, v)?)),
            LuaKey::Num(_) => None,
        })
        .collect();
    LuaVal::Table(Rc::new(LuaTab { arr: Vec::new(), fields }))
}

/// Each value of a table through `f`, every key and position kept: the sizes and the orders of `groups` and `nodes` decide `pairs` (`constructor_pairs`).
fn each_value(t: &LuaTab, f: &dyn Fn(&LuaVal) -> LuaVal) -> LuaVal {
    LuaVal::Table(Rc::new(LuaTab {
        arr: t.arr.iter().map(f).collect(),
        fields: t.fields.iter().map(|(k, v)| (k.clone(), f(v))).collect(),
    }))
}

fn ascendancy(v: &LuaVal) -> LuaVal {
    let Some(a) = v.as_tab() else { return v.clone() };
    with_fields(a, |k, x| {
        matches!(k, "id" | "flavourText" | "flavourTextColour" | "flavourTextRect").then(|| x.clone())
    })
}

fn class(v: &LuaVal) -> LuaVal {
    let Some(c) = v.as_tab() else { return v.clone() };
    with_fields(c, |k, x| {
        (k == "ascendancies").then(|| x.as_tab().map_or_else(|| x.clone(), |t| each_value(t, &ascendancy)))
    })
}

fn node(v: &LuaVal) -> LuaVal {
    let Some(n) = v.as_tab() else { return v.clone() };
    with_fields(n, |k, x| match k {
        "skill" | "id" | "icon" | "activeIcon" | "inactiveIcon" | "activeEffectImage" | "recipe" | "flavourText" | "out" => Some(x.clone()),
        // only whether it is there is read
        "masteryEffects" => Some(LuaVal::Bool(true)),
        _ => None,
    })
}

fn group(v: &LuaVal) -> LuaVal {
    let Some(g) = v.as_tab() else { return v.clone() };
    with_fields(g, |k, x| matches!(k, "orbits" | "oo").then(|| x.clone()))
}

/// What `TreeArt::load` reads of a `tree.lua`, in source order. The rest (the stats of the nodes, above all) is what `data/tree/*.json` holds for the calculation.
pub fn project_tree(v: &LuaVal) -> Result<LuaVal, String> {
    let t = v.as_tab().ok_or("tree.lua is not a table")?;
    Ok(with_fields(t, |k, x| match k {
        // only the names of the assets are read
        "assets" => x.as_tab().map(|a| each_value(a, &|_| LuaVal::Bool(true))),
        "skillSprites" | "sprites" | "imageZoomLevels" | "min_x" | "min_y" | "max_x" | "max_y" => Some(x.clone()),
        "alternate_ascendancies" => x.as_tab().map(|a| each_value(a, &ascendancy)),
        "classes" => x.as_tab().map(|a| each_value(a, &class)),
        "groups" => x.as_tab().map(|a| each_value(a, &group)),
        "nodes" => x.as_tab().map(|a| each_value(a, &node)),
        _ => None,
    }))
}

fn parse_file(path: &Path) -> Result<LuaVal, String> {
    let src = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_return(&src).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every file under `dir`, `/`-separated relative to it, sorted.
fn walk(dir: &Path) -> Result<Vec<String>, String> {
    fn go(root: &Path, at: &Path, out: &mut Vec<String>) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", at.display());
        for e in std::fs::read_dir(at).map_err(err)? {
            let p = e.map_err(err)?.path();
            if p.is_dir() {
                go(root, &p, out)?;
            } else {
                out.push(p.strip_prefix(root).expect("under the root").to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    go(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// `root/` -> the tables of `src.cache` but the copies: `(key, value)`.
pub fn tables(root: &str) -> Result<Vec<(String, LuaVal)>, String> {
    let mut out = Vec::new();
    let tree_data = Path::new(root).join("TreeData");
    for rel in walk(&tree_data)? {
        if !rel.ends_with(".lua") {
            continue;
        }
        let v = parse_file(&tree_data.join(&rel))?;
        let v = if rel.ends_with("/tree.lua") { project_tree(&v)? } else { v };
        out.push((format!("TreeData/{rel}"), v));
    }
    let mut files: Vec<String> = DATA_TABLES.iter().map(|s| (*s).to_string()).collect();
    for rel in walk(&Path::new(root).join("Data/StatDescriptions"))? {
        if rel.ends_with(".lua") && !rel.contains('/') {
            files.push(format!("Data/StatDescriptions/{rel}"));
        }
    }
    for rel in files {
        let v = parse_file(&Path::new(root).join(&rel))?;
        out.push((rel, v));
    }
    for (key, make) in products() {
        out.push((key.to_string(), make(root)?));
    }
    for rel in POPUP_TABLES {
        out.push((popup_key(rel), json_to_lua(&popup_table(root, rel)?)));
    }
    Ok(out)
}

/// Which of the files of one content stays: a file directly in `TreeData/` (the readers of those are the most), then `legion/`, then the newest version; among those the first name.
fn canonical(paths: &[String]) -> usize {
    let rank = |p: &str| -> (u8, std::cmp::Reverse<String>, String) {
        let (dir, _) = p.rsplit_once('/').unwrap_or(("", p));
        let order = match dir {
            "" => 0,
            "legion" => 1,
            _ => 2,
        };
        (order, std::cmp::Reverse(dir.to_string()), p.to_string())
    };
    (0..paths.len()).min_by_key(|&i| rank(&paths[i])).expect("a group is not empty")
}

/// What [`generate`] did.
#[derive(Debug, Default)]
pub struct Stats {
    pub files_in: usize,
    pub bytes_in: u64,
    pub unique: usize,
    pub unique_bytes: u64,
    pub copies: usize,
    pub tables: usize,
    pub cache_bytes: u64,
    pub tree_lua_text: u64,
    pub tree_lua_full: u64,
    pub tree_lua_projected: u64,
}

/// Writes `<out>/TreeData/` (one file per content, no `.lua`) and `<out>/src.cache` from `root/` of upstream.
pub fn generate(root: &str, out: &Path, manifest_sha256: &str) -> Result<Stats, String> {
    let tree_data = Path::new(root).join("TreeData");
    let mut st = Stats::default();
    // content -> the paths that have it (sha256: "copies" are the same bytes, and `verify` compares the bytes)
    let mut groups: BTreeMap<(u64, String), Vec<String>> = BTreeMap::new();
    for rel in walk(&tree_data)? {
        if rel.ends_with(".lua") {
            continue;
        }
        let bytes = std::fs::read(tree_data.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        st.files_in += 1;
        st.bytes_in += bytes.len() as u64;
        groups
            .entry((bytes.len() as u64, pob_data::sha256::sha256_hex(&bytes)))
            .or_default()
            .push(rel);
    }
    let mut copies = Vec::new();
    for ((len, _), paths) in &groups {
        let keep = &paths[canonical(paths)];
        let to = out.join("TreeData").join(keep);
        std::fs::create_dir_all(to.parent().expect("a file has a folder")).map_err(|e| format!("{}: {e}", to.display()))?;
        std::fs::copy(tree_data.join(keep), &to).map_err(|e| format!("{keep}: {e}"))?;
        st.unique += 1;
        st.unique_bytes += len;
        for p in paths {
            if p != keep {
                let rec = LuaTab {
                    arr: vec![LuaVal::Str(format!("TreeData/{keep}").into()), LuaVal::Num(*len as f64)],
                    fields: Vec::new(),
                };
                copies.push((
                    LuaKey::Str(format!("treedata/{}", p.to_ascii_lowercase()).into()),
                    LuaVal::Table(Rc::new(rec)),
                ));
            }
        }
    }
    st.copies = copies.len();
    let mut encoded = vec![(
        COPIES.to_string(),
        encode(&LuaVal::Table(Rc::new(LuaTab {
            arr: Vec::new(),
            fields: copies,
        }))),
    )];
    for (key, v) in tables(root)? {
        if key.ends_with("/tree.lua") {
            let path = Path::new(root).join(&key);
            st.tree_lua_text += std::fs::metadata(&path).map_err(|e| format!("{key}: {e}"))?.len();
            st.tree_lua_full += encode(&parse_file(&path)?).len() as u64;
        }
        let bytes = encode(&v);
        if key.ends_with("/tree.lua") {
            st.tree_lua_projected += bytes.len() as u64;
        }
        encoded.push((key, bytes));
    }
    st.tables = encoded.len();
    let file = out.join(FILE);
    write_tables(MAGIC, &encoded, manifest_sha256, &file).map_err(|e| e.0)?;
    st.cache_bytes = std::fs::metadata(&file).map_err(|e| format!("{}: {e}", file.display()))?.len();
    Ok(st)
}

/// Every file and every table of the package `<out>/` against `root/` of upstream, through the readers' own way there: [`Package`]. An error names what differs.
pub fn verify(root: &str, out: &Path, manifest_sha256: &str) -> Result<(usize, usize), String> {
    let out_root = format!("{}/", out.to_string_lossy().replace('\\', "/"));
    let p = Package::open(&out_root, manifest_sha256);
    if !matches!(p, Package::Open { .. }) {
        return Err(format!("{}: not the cache of this data", out.join(FILE).display()));
    }
    let lua: Vec<String> = walk(out)?
        .into_iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".lua"))
        .collect();
    if !lua.is_empty() {
        return Err(format!("the package has {} Lua files: {}", lua.len(), lua[0]));
    }
    // the files: bytes through the table = the bytes of upstream
    let mut n_files = 0;
    let tree_data = Path::new(root).join("TreeData");
    let mut read = BTreeSet::new();
    for rel in walk(&tree_data)? {
        if rel.ends_with(".lua") {
            continue;
        }
        let key = format!("TreeData/{rel}");
        let path = p.resolve(&out_root, &key)?.unwrap_or_else(|| format!("{out_root}{key}"));
        let got = std::fs::read(&path).map_err(|e| format!("{key}: {path}: {e}"))?;
        let want = std::fs::read(tree_data.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        if got != want {
            return Err(format!("{key}: {path} has other bytes than upstream's file"));
        }
        read.insert(path);
        n_files += 1;
    }
    // no file beyond those: the copies are gone
    for rel in walk(&out.join("TreeData"))? {
        if !read.contains(&format!("{out_root}TreeData/{rel}")) {
            return Err(format!("TreeData/{rel}: no reader gets to this file"));
        }
    }
    // the tables: the value through the cache = the value of the text, order and bits
    let mut n_tables = 0;
    for (key, want) in tables(root)? {
        let Some(got) = p.table(&key) else {
            return Err(format!("{FILE}: no table '{key}'"));
        };
        if !same(&got?, &want) {
            return Err(format!("{FILE}: the table '{key}' differs from the text"));
        }
        n_tables += 1;
    }
    Ok((n_files, n_tables))
}
