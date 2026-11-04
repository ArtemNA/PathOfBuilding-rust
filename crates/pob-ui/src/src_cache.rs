//! `src/src.cache` of a package: what the UI reads from upstream's `src/` when the `.lua` files and the copies of the tree's images are not there.
//!
//! * A table is a [`LuaVal`] in binary form: positional items and keyed fields in source order, every number bit for bit. The order decides `pairs` (`LuaTab`, MINEFIELD D13), so
//!   it is kept as the parser saw it. The container is the one of `data.cache` (`pob_data::bincache`): the sha of `data.lock` in the header, a checksum per table.
//! * Keys: `<rel>` is the file's table (`Data/QueryMods.lua`), `<rel>#<what>` is what a reader that scans the raw text gets out of it, and [`COPIES`] is "path -> canonical path" of
//!   the tree's files. The makers of those live next to their readers: a reader in a checkout goes through the same maker, so the two paths cannot read differently.
//! * A checkout has no such file: everything reads the text, and [`resolve`] changes nothing. A package has no text: a missing, stale or damaged cache is an error, never less data.
//!   The cache beside the text is ignored when it is stale, as `data.cache` is, and an error when it is damaged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use pob_data::bincache::{CacheFile, get_var, put_var};

use crate::lua_data::{LuaKey, LuaTab, LuaVal, parse_return};

/// `<src>/src.cache`.
pub const FILE: &str = "src.cache";
/// Bump the digit when the form of a table changes: an older file is then foreign.
pub const MAGIC: &[u8; 8] = b"POBSRC1\n";
/// The table of the copies: `<lowercase path under src/>` -> `{ <canonical path>, <size> }`.
pub const COPIES: &str = "@copies";

const T_NIL: u8 = 0;
const T_FALSE: u8 = 1;
const T_TRUE: u8 = 2;
const T_UINT: u8 = 3;
const T_FLOAT: u8 = 4;
const T_STR: u8 = 5;
const T_TABLE: u8 = 6;

// --- the form of a table ---------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Encoder {
    strings: HashMap<Rc<str>, u64>,
    table: Vec<u8>,
    body: Vec<u8>,
}

impl Encoder {
    fn string(&mut self, s: &Rc<str>) {
        let next = self.strings.len() as u64;
        let i = *self.strings.entry(s.clone()).or_insert_with(|| {
            put_var(&mut self.table, s.len() as u64);
            self.table.extend_from_slice(s.as_bytes());
            next
        });
        put_var(&mut self.body, i);
    }

    fn number(&mut self, n: f64) {
        // the small non-negative integers of tree.lua; everything else keeps its bits (-0.0 fails the comparison of bits)
        let u = n as u64;
        if (u as f64).to_bits() == n.to_bits() {
            self.body.push(T_UINT);
            put_var(&mut self.body, u);
        } else {
            self.body.push(T_FLOAT);
            self.body.extend_from_slice(&n.to_le_bytes());
        }
    }

    fn value(&mut self, v: &LuaVal) {
        match v {
            LuaVal::Nil => self.body.push(T_NIL),
            LuaVal::Bool(false) => self.body.push(T_FALSE),
            LuaVal::Bool(true) => self.body.push(T_TRUE),
            LuaVal::Num(n) => self.number(*n),
            LuaVal::Str(s) => {
                self.body.push(T_STR);
                self.string(s);
            }
            LuaVal::Table(t) => {
                self.body.push(T_TABLE);
                put_var(&mut self.body, t.arr.len() as u64);
                put_var(&mut self.body, t.fields.len() as u64);
                for x in &t.arr {
                    self.value(x);
                }
                for (k, x) in &t.fields {
                    match k {
                        LuaKey::Num(n) => self.number(*n),
                        LuaKey::Str(s) => {
                            self.body.push(T_STR);
                            self.string(s);
                        }
                    }
                    self.value(x);
                }
            }
        }
    }
}

/// A value as a table of the cache.
pub fn encode(v: &LuaVal) -> Vec<u8> {
    let mut e = Encoder::default();
    e.value(v);
    let mut out = Vec::with_capacity(e.table.len() + e.body.len() + 8);
    put_var(&mut out, e.strings.len() as u64);
    out.append(&mut e.table);
    out.append(&mut e.body);
    out
}

struct Decoder<'a> {
    b: &'a [u8],
    i: usize,
    strings: Vec<Rc<str>>,
}

impl Decoder<'_> {
    fn var(&mut self) -> Result<u64, String> {
        get_var(self.b, &mut self.i).ok_or_else(|| "a table ends early".to_string())
    }

    fn string(&mut self) -> Result<Rc<str>, String> {
        let i = self.var()? as usize;
        self.strings.get(i).cloned().ok_or_else(|| format!("no string {i}"))
    }

    fn float(&mut self) -> Result<f64, String> {
        let w = self.b.get(self.i..self.i + 8).ok_or("a table ends early")?;
        self.i += 8;
        Ok(f64::from_le_bytes(w.try_into().map_err(|_| "a table ends early")?))
    }

    fn tag(&mut self) -> Result<u8, String> {
        let t = *self.b.get(self.i).ok_or("a table ends early")?;
        self.i += 1;
        Ok(t)
    }

    fn value(&mut self) -> Result<LuaVal, String> {
        Ok(match self.tag()? {
            T_NIL => LuaVal::Nil,
            T_FALSE => LuaVal::Bool(false),
            T_TRUE => LuaVal::Bool(true),
            T_UINT => LuaVal::Num(self.var()? as f64),
            T_FLOAT => LuaVal::Num(self.float()?),
            T_STR => LuaVal::Str(self.string()?),
            T_TABLE => {
                let (na, nf) = (self.var()? as usize, self.var()? as usize);
                // at least a byte each: a damaged count does not ask for all the memory
                if na.saturating_add(nf) > self.b.len() {
                    return Err("a table ends early".to_string());
                }
                let mut t = LuaTab {
                    arr: Vec::with_capacity(na),
                    fields: Vec::with_capacity(nf),
                };
                for _ in 0..na {
                    t.arr.push(self.value()?);
                }
                for _ in 0..nf {
                    let k = match self.tag()? {
                        T_UINT => LuaKey::Num(self.var()? as f64),
                        T_FLOAT => LuaKey::Num(self.float()?),
                        T_STR => LuaKey::Str(self.string()?),
                        t => return Err(format!("the key tag {t}")),
                    };
                    t.fields.push((k, self.value()?));
                }
                LuaVal::Table(Rc::new(t))
            }
            t => return Err(format!("the tag {t}")),
        })
    }
}

/// A table from [`encode`]'s bytes.
pub fn decode(bytes: &[u8]) -> Result<LuaVal, String> {
    let mut d = Decoder {
        b: bytes,
        i: 0,
        strings: Vec::new(),
    };
    let n = d.var()? as usize;
    if n > bytes.len() {
        return Err("a table ends early".to_string());
    }
    d.strings.reserve(n);
    for _ in 0..n {
        let len = d.var()? as usize;
        let s = bytes.get(d.i..d.i.saturating_add(len)).ok_or("a table ends early")?;
        d.strings.push(std::str::from_utf8(s).map_err(|e| e.to_string())?.into());
        d.i += len;
    }
    let v = d.value()?;
    if d.i != bytes.len() {
        return Err(format!("{} bytes after the table", bytes.len() - d.i));
    }
    Ok(v)
}

/// The same value in the same order, every number bit for bit.
pub fn same(a: &LuaVal, b: &LuaVal) -> bool {
    match (a, b) {
        (LuaVal::Nil, LuaVal::Nil) => true,
        (LuaVal::Bool(x), LuaVal::Bool(y)) => x == y,
        (LuaVal::Num(x), LuaVal::Num(y)) => x.to_bits() == y.to_bits(),
        (LuaVal::Str(x), LuaVal::Str(y)) => x == y,
        (LuaVal::Table(x), LuaVal::Table(y)) => {
            x.arr.len() == y.arr.len()
                && x.fields.len() == y.fields.len()
                && x.arr.iter().zip(&y.arr).all(|(p, q)| same(p, q))
                && x.fields.iter().zip(&y.fields).all(|((k, p), (l, q))| {
                    (match (k, l) {
                        (LuaKey::Num(m), LuaKey::Num(n)) => m.to_bits() == n.to_bits(),
                        (LuaKey::Str(m), LuaKey::Str(n)) => m == n,
                        _ => false,
                    }) && same(p, q)
                })
        }
        _ => false,
    }
}

// --- reading ----------------------------------------------------------------------------------------------------------------

/// What is known of the `src/` of a root.
pub enum Package {
    /// No `src.cache`: a checkout, or a package that lost it.
    None,
    /// There is a file, but not for this data (or not of this form).
    Stale,
    Open {
        cache: CacheFile,
        copies: OnceLock<Result<Copies, String>>,
    },
}

/// `<lowercase path under src/>` -> (canonical path, its size).
pub type Copies = HashMap<String, (String, u64)>;

impl Package {
    /// `manifest_sha256`: of `data.lock`.
    pub fn open(root: &str, manifest_sha256: &str) -> Package {
        let path = Path::new(root).join(FILE);
        if !path.exists() {
            return Package::None;
        }
        match CacheFile::open_as(&path, MAGIC, manifest_sha256) {
            Some(cache) => Package::Open {
                cache,
                copies: OnceLock::new(),
            },
            None => Package::Stale,
        }
    }

    /// The table `key`: `None` if this cache has no say in it.
    pub fn table(&self, key: &str) -> Option<Result<LuaVal, String>> {
        let Package::Open { cache, .. } = self else { return None };
        Some(
            cache
                .raw(key)?
                .and_then(|b| decode(&b).map_err(|e| format!("{FILE}: '{key}': {e}: the file is damaged"))),
        )
    }

    pub fn copies(&self) -> Result<Option<&Copies>, String> {
        let Package::Open { cache, copies } = self else { return Ok(None) };
        let c = copies.get_or_init(|| {
            let Some(raw) = cache.raw(COPIES) else {
                return Err(format!("{FILE}: no table {COPIES}"));
            };
            let v = decode(&raw?)?;
            let mut out = HashMap::new();
            for (k, v) in &v.as_tab().ok_or("the copies are not a table")?.fields {
                let (LuaKey::Str(k), Some(rec)) = (k, v.as_tab()) else {
                    return Err(format!("{FILE}: a bad record in {COPIES}"));
                };
                let (Some(to), Some(len)) = (rec.get_num(1.0).and_then(LuaVal::as_str), rec.get_num(2.0).and_then(LuaVal::as_num)) else {
                    return Err(format!("{FILE}: a bad record of '{k}' in {COPIES}"));
                };
                out.insert(k.to_string(), (to.to_string(), len as u64));
            }
            Ok(out)
        });
        c.as_ref().map(Some).map_err(Clone::clone)
    }

    /// The canonical file of `rel` under `root`. A record that names a file of another size stops here: without this a wrong record would only be an image of the wrong size, or none.
    pub fn resolve(&self, root: &str, rel: &str) -> Result<Option<String>, String> {
        let Some(copies) = self.copies()? else { return Ok(None) };
        let Some((to, len)) = copies.get(&rel.to_ascii_lowercase()) else {
            return Ok(None);
        };
        let path = format!("{root}{to}");
        let got = std::fs::metadata(&path)
            .map_err(|e| format!("{FILE}: '{rel}' is a copy of '{to}': {e}"))?
            .len();
        if got != *len {
            return Err(format!("{FILE}: '{rel}' is a copy of '{to}' of {len} bytes, the file has {got}"));
        }
        Ok(Some(path))
    }
}

/// The package of a root, once per process and root. The tests of the damaged files use [`Package::open`].
fn package(root: &str) -> Arc<Package> {
    static OPENED: OnceLock<Mutex<HashMap<String, Arc<Package>>>> = OnceLock::new();
    let mut m = OPENED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    m.entry(root.to_string())
        .or_insert_with(|| {
            // Without the lock nothing can be said to match; the data check of the start reports it
            let dir = pob_data::data_dir();
            Arc::new(match pob_data::verify_lock(&dir, &pob_data::lock_path(&dir)) {
                Ok(lock) => Package::open(root, &lock.manifest_sha256),
                Err(_) => Package::Stale,
            })
        })
        .clone()
}

/// A checkout has the text; a package does not, and then the cache is the only source.
fn has_text(root: &str) -> bool {
    Path::new(root).join("GameVersions.lua").is_file()
}

fn no_cache(root: &str, what: &str, p: &Package) -> String {
    let why = match p {
        Package::None => "there is no",
        _ => "this is not the",
    };
    format!(
        "{what}: {root} has no Lua files and {why} {FILE} of this version of the application: its files are damaged or come from different versions, install it again"
    )
}

/// `dofile` of `<root><rel>`: the table of the cache when there is one, the text otherwise.
pub fn read_lua(root: &str, rel: &str) -> Result<LuaVal, String> {
    let p = package(root);
    if let Some(v) = p.table(rel) {
        return v;
    }
    let path = format!("{root}{rel}");
    if !has_text(root) {
        return Err(no_cache(root, rel, &p));
    }
    let src = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    parse_return(&src).map_err(|e| format!("{path}: {e}"))
}

/// What a reader of the raw text gets out of it: the table `key` of the cache, or `make` over the text.
pub fn product(root: &str, key: &str, make: impl FnOnce(&str) -> Result<LuaVal, String>) -> Result<LuaVal, String> {
    let p = package(root);
    if let Some(v) = p.table(key) {
        return v;
    }
    if !has_text(root) {
        return Err(no_cache(root, key, &p));
    }
    make(root)
}

/// The names of the `.lua` files directly under `<root><dir>/`, without `.lua`, in no given order.
pub fn lua_files(root: &str, dir: &str) -> Vec<String> {
    let p = package(root);
    if let Package::Open { cache, .. } = &*p {
        let prefix = format!("{dir}/");
        return cache
            .table_names()
            .filter_map(|n| n.strip_prefix(&prefix)?.strip_suffix(".lua"))
            .filter(|n| !n.contains('/'))
            .map(str::to_string)
            .collect();
    }
    std::fs::read_dir(format!("{root}{dir}"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some(e.file_name().to_string_lossy().strip_suffix(".lua")?.to_string()))
        .collect()
}

/// The file to read for `<root><rel>`: its canonical copy in a package, itself otherwise.
pub fn resolve(root: &str, rel: &str) -> String {
    match package(root).resolve(root, rel) {
        Ok(Some(path)) => path,
        Ok(None) => format!("{root}{rel}"),
        Err(e) => panic!("{e}"),
    }
}

/// `<root><rel>` is a file, or was before its copies were dropped.
pub fn is_file(root: &str, rel: &str) -> bool {
    Path::new(&resolve(root, rel)).is_file()
}

/// The check of the start: `src` of the package beside `data` can be read. A checkout has no `src` there.
pub fn check(data_dir: &Path) -> Result<(), String> {
    let src: PathBuf = data_dir.join("..").join("src");
    if !src.join("TreeData").is_dir() {
        return Ok(());
    }
    let root = format!("{}/", src.to_string_lossy().replace('\\', "/"));
    if has_text(&root) {
        return Ok(());
    }
    let p = package(&root);
    match &*p {
        Package::Open { .. } => p.copies().map(|_| ()),
        _ => Err(no_cache(&root, FILE, &p)),
    }
}
