//! `data.cache` beside `data/` (RUST-ARCH §8, R3-cache): the JSON tables in a binary form that reads faster, made by [`write_cache`] from the files themselves.
//!
//! * The form is the JSON tree itself: one tagged value per JSON value, objects in file order, numbers as the JSON parser gave them. [`Value`] deserializes into any
//!   type the JSON does, so a table has one model and no second conversion to get wrong (the gate: `tests/bincache.rs`).
//! * The header carries `manifestSha256` of `data.lock`: a missing, stale, truncated or foreign file means the JSON (`read_json`). `POB_DATA_NO_CACHE` turns it off.
//! * Layout: magic, format, sha, table count, then per table name / offset / length / checksum; each table is its string table followed by the root value.

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::de::{self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess, Visitor};

use crate::DataError;

const MAGIC: &[u8; 8] = b"POBDATA\n";
/// Bump when the form changes: an older file is then stale.
const FORMAT: u32 = 2;

/// A damaged table is the JSON too, not a panic on the row a lazy reader reaches later. Not a security check: the file is the user's.
pub fn checksum(bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ bytes.len() as u64;
    let (words, rest) = bytes.as_chunks::<8>();
    for w in words {
        h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(PRIME).rotate_left(29);
    }
    for b in rest {
        h = (h ^ u64::from(*b)).wrapping_mul(PRIME);
    }
    h
}

const T_NULL: u8 = 0;
const T_FALSE: u8 = 1;
const T_TRUE: u8 = 2;
const T_UINT: u8 = 3;
const T_INT: u8 = 4;
const T_FLOAT: u8 = 5;
const T_STR: u8 = 6;
const T_ARRAY: u8 = 7;
const T_OBJECT: u8 = 8;

/// `data.cache` next to `data.lock`.
pub fn cache_path(dir: &Path) -> PathBuf {
    dir.join("..").join("data.cache")
}

// --- writing --------------------------------------------------------------------------------------------------------------

pub fn put_var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

#[derive(Default)]
struct Encoder {
    strings: HashMap<String, u64>,
    table: Vec<u8>,
    body: Vec<u8>,
}

impl Encoder {
    fn string(&mut self, s: &str) -> u64 {
        if let Some(&i) = self.strings.get(s) {
            return i;
        }
        let i = self.strings.len() as u64;
        self.strings.insert(s.to_owned(), i);
        put_var(&mut self.table, s.len() as u64);
        self.table.extend_from_slice(s.as_bytes());
        i
    }

    /// The JSON text as a table: walked by the JSON parser, so the order and every number are what `read_json` would see.
    fn encode(mut self, text: &str) -> Result<Vec<u8>, serde_json::Error> {
        let mut de = serde_json::Deserializer::from_str(text);
        de::Deserializer::deserialize_any(&mut de, Enc(&mut self))?;
        de.end()?;
        let mut out = Vec::with_capacity(self.table.len() + self.body.len() + 8);
        put_var(&mut out, self.strings.len() as u64);
        out.append(&mut self.table);
        out.append(&mut self.body);
        Ok(out)
    }
}

/// Writes one JSON value into the encoder's body.
struct Enc<'a>(&'a mut Encoder);

/// A JSON string, as a table reference. Keys go this way too.
struct EncStr<'a>(&'a mut Encoder);

impl<'de> Visitor<'de> for EncStr<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a key")
    }
    fn visit_str<E: de::Error>(self, s: &str) -> Result<(), E> {
        let i = self.0.string(s);
        put_var(&mut self.0.body, i);
        Ok(())
    }
}

impl<'de> DeserializeSeed<'de> for EncStr<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_str(self)
    }
}

impl<'de> DeserializeSeed<'de> for Enc<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Enc<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_unit<E>(self) -> Result<(), E> {
        self.0.body.push(T_NULL);
        Ok(())
    }
    fn visit_bool<E>(self, b: bool) -> Result<(), E> {
        self.0.body.push(if b { T_TRUE } else { T_FALSE });
        Ok(())
    }
    fn visit_u64<E>(self, v: u64) -> Result<(), E> {
        self.0.body.push(T_UINT);
        put_var(&mut self.0.body, v);
        Ok(())
    }
    fn visit_i64<E>(self, v: i64) -> Result<(), E> {
        self.0.body.push(T_INT);
        self.0.body.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn visit_f64<E>(self, v: f64) -> Result<(), E> {
        self.0.body.push(T_FLOAT);
        self.0.body.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn visit_str<E: de::Error>(self, s: &str) -> Result<(), E> {
        let i = self.0.string(s);
        self.0.body.push(T_STR);
        put_var(&mut self.0.body, i);
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        // the elements go to their own buffer: their count comes first
        let outer = std::mem::take(&mut self.0.body);
        let mut n = 0u64;
        while a.next_element_seed(Enc(self.0))?.is_some() {
            n += 1;
        }
        let inner = std::mem::replace(&mut self.0.body, outer);
        self.0.body.push(T_ARRAY);
        put_var(&mut self.0.body, n);
        self.0.body.extend_from_slice(&inner);
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let outer = std::mem::take(&mut self.0.body);
        let mut n = 0u64;
        while a.next_key_seed(EncStr(self.0))?.is_some() {
            a.next_value_seed(Enc(self.0))?;
            n += 1;
        }
        let inner = std::mem::replace(&mut self.0.body, outer);
        self.0.body.push(T_OBJECT);
        put_var(&mut self.0.body, n);
        self.0.body.extend_from_slice(&inner);
        Ok(())
    }
}

/// One JSON text as a table of the cache.
pub fn encode_table(text: &str) -> Result<Vec<u8>, DataError> {
    Encoder::default()
        .encode(text)
        .map_err(|e| DataError(format!("pob-data: data.cache: {e}")))
}

/// Every `*.json` under `dir` but the manifest and the rarely read `tree/_ondemand` (a path not in the cache reads the JSON), `/`-separated, sorted.
pub fn table_files(dir: &Path) -> Result<Vec<String>, DataError> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) -> Result<(), DataError> {
        let err = |e: std::io::Error| DataError(format!("pob-data: {}: {e}", at.display()));
        for f in std::fs::read_dir(at).map_err(err)? {
            let p = f.map_err(err)?.path();
            let rel = p.strip_prefix(root).expect("under the root").to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                if rel != "tree/_ondemand" {
                    walk(root, &p, out)?;
                }
            } else if rel.ends_with(".json") && rel != "manifest.json" {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Writes the cache of `dir` for `manifest_sha256` to `out` (through a temporary file beside it). Returns how many tables it holds.
pub fn write_cache(dir: &Path, manifest_sha256: &str, out: &Path) -> Result<usize, DataError> {
    let files = table_files(dir)?;
    let mut tables = Vec::with_capacity(files.len());
    for rel in files {
        let path = dir.join(&rel);
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        let table = encode_table(&text).map_err(|e| DataError(format!("{rel}: {e}")))?;
        tables.push((rel, table));
    }
    write_tables(MAGIC, &tables, manifest_sha256, out)?;
    Ok(tables.len())
}

/// A cache file of `tables` (name, bytes) for `manifest_sha256`, written through a temporary file beside `out`: the layout of `data.cache`, whatever the form of the tables is.
pub fn write_tables(magic: &[u8; 8], tables: &[(String, Vec<u8>)], manifest_sha256: &str, out: &Path) -> Result<(), DataError> {
    let mut dirent = Vec::new();
    let head_len = magic.len() + 4 + 4 + 8 + 4 + manifest_sha256.len();
    let dir_len: usize = tables.iter().map(|(r, _)| 4 + r.len() + 24).sum();
    let mut offset = (head_len + dir_len) as u64;
    for (rel, t) in tables {
        dirent.extend_from_slice(&(rel.len() as u32).to_le_bytes());
        dirent.extend_from_slice(rel.as_bytes());
        dirent.extend_from_slice(&offset.to_le_bytes());
        dirent.extend_from_slice(&(t.len() as u64).to_le_bytes());
        dirent.extend_from_slice(&checksum(t).to_le_bytes());
        offset += t.len() as u64;
    }
    let mut bytes = Vec::with_capacity(offset as usize);
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    bytes.extend_from_slice(&(manifest_sha256.len() as u32).to_le_bytes());
    bytes.extend_from_slice(manifest_sha256.as_bytes());
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&(tables.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&dirent);
    for (_, t) in tables {
        bytes.extend_from_slice(t);
    }
    let tmp = out.with_extension("cache.tmp");
    let io = |e: std::io::Error| DataError(format!("pob-data: {}: {e}", out.display()));
    std::fs::write(&tmp, &bytes).map_err(io)?;
    std::fs::rename(&tmp, out).map_err(io)
}

// --- reading --------------------------------------------------------------------------------------------------------------

/// An opened cache: its directory; the tables are read when asked for.
#[derive(Debug)]
pub struct CacheFile {
    path: PathBuf,
    tables: HashMap<String, (u64, u64, u64)>,
}

impl CacheFile {
    /// `None`: not there, or not the cache of `manifest_sha256` (the JSON then).
    pub fn open(path: &Path, manifest_sha256: &str) -> Option<CacheFile> {
        Self::open_as(path, MAGIC, manifest_sha256)
    }

    /// [`Self::open`] of a cache whose tables have another form, told apart by `want_magic`.
    pub fn open_as(path: &Path, want_magic: &[u8; 8], manifest_sha256: &str) -> Option<CacheFile> {
        let mut f = std::fs::File::open(path).ok()?;
        let file_len = f.metadata().ok()?.len();
        let mut head = vec![0u8; want_magic.len() + 8 + manifest_sha256.len() + 8 + 4];
        f.read_exact(&mut head).ok()?;
        let (magic, rest) = head.split_at(want_magic.len());
        if magic != want_magic {
            return None;
        }
        let u32_at = |b: &[u8], i: usize| b[i..i + 4].try_into().ok().map(u32::from_le_bytes);
        let u64_at = |b: &[u8], i: usize| b[i..i + 8].try_into().ok().map(u64::from_le_bytes);
        if u32_at(rest, 0)? != FORMAT || u32_at(rest, 4)? as usize != manifest_sha256.len() {
            return None;
        }
        let rest = &rest[8..];
        if &rest[..manifest_sha256.len()] != manifest_sha256.as_bytes() {
            return None;
        }
        let rest = &rest[manifest_sha256.len()..];
        if u64_at(rest, 0)? != file_len {
            return None;
        }
        let count = u32_at(rest, 8)? as usize;
        let mut tables = HashMap::with_capacity(count);
        for _ in 0..count {
            let mut b = [0u8; 4];
            f.read_exact(&mut b).ok()?;
            let mut name = vec![0u8; u32::from_le_bytes(b) as usize];
            f.read_exact(&mut name).ok()?;
            let mut pos = [0u8; 24];
            f.read_exact(&mut pos).ok()?;
            let (at, len, sum) = (u64_at(&pos, 0)?, u64_at(&pos, 8)?, u64_at(&pos, 16)?);
            if at.checked_add(len)? > file_len {
                return None;
            }
            tables.insert(String::from_utf8(name).ok()?, (at, len, sum));
        }
        Some(CacheFile {
            path: path.to_path_buf(),
            tables,
        })
    }

    /// The table of `<data>/<rel>`, if the cache has it and it can be read.
    pub fn table(&self, rel: &str) -> Option<Table> {
        Table::new(self.raw(rel)?.ok()?)
    }

    /// The bytes of a table: `None` if the cache has no such table, an error if it cannot be read or its checksum differs.
    pub fn raw(&self, rel: &str) -> Option<Result<Vec<u8>, String>> {
        let &(at, len, sum) = self.tables.get(rel)?;
        let read = || -> Result<Vec<u8>, String> {
            let io = |e: std::io::Error| format!("{}: {e}", self.path.display());
            let mut f = std::fs::File::open(&self.path).map_err(io)?;
            f.seek(SeekFrom::Start(at)).map_err(io)?;
            let mut bytes = vec![0u8; len as usize];
            f.read_exact(&mut bytes).map_err(io)?;
            if checksum(&bytes) != sum {
                return Err(format!("{}: the checksum of '{rel}' differs: the file is damaged", self.path.display()));
            }
            Ok(bytes)
        };
        Some(read())
    }

    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.keys().map(String::as_str)
    }
}

/// The cache of this process, once. Checked against the lock like the JSON.
pub(crate) fn cache() -> Option<&'static CacheFile> {
    static C: OnceLock<Option<CacheFile>> = OnceLock::new();
    C.get_or_init(|| {
        if std::env::var_os("POB_DATA_NO_CACHE").is_some() {
            return None;
        }
        let dir = crate::data_dir();
        let lock = crate::verify_lock(&dir, &crate::lock_path(&dir)).ok()?;
        CacheFile::open(&cache_path(&dir), &lock.manifest_sha256)
    })
    .as_ref()
}

/// One table in memory: its bytes and where its strings are.
#[derive(Debug)]
pub struct Table {
    bytes: Vec<u8>,
    strings: Vec<(u32, u32)>,
    root: usize,
}

pub fn get_var(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let (mut v, mut shift) = (0u64, 0u32);
    loop {
        let b = *bytes.get(*pos)?;
        *pos += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b < 0x80 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

#[derive(Debug)]
pub struct DecodeError(String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

impl de::Error for DecodeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DecodeError(msg.to_string())
    }
}

fn truncated() -> DecodeError {
    DecodeError("data.cache: a table ends early".to_owned())
}

impl Table {
    /// A table from [`encode_table`]'s bytes. `None`: damaged.
    pub fn new(bytes: Vec<u8>) -> Option<Table> {
        let mut pos = 0;
        let n = get_var(&bytes, &mut pos)? as usize;
        let mut strings = Vec::with_capacity(n);
        for _ in 0..n {
            let len = get_var(&bytes, &mut pos)? as usize;
            strings.push((pos as u32, len as u32));
            pos = pos.checked_add(len)?;
        }
        (pos <= bytes.len()).then_some(Table { bytes, strings, root: pos })
    }

    /// The table as a `T`, as if from its JSON.
    pub fn read<T: DeserializeOwned>(&self) -> Result<T, DecodeError> {
        self.read_at(self.root)
    }

    /// The value at `pos` (a position from [`Self::array_positions`]) as a `T`.
    pub fn read_at<T: DeserializeOwned>(&self, pos: usize) -> Result<T, DecodeError> {
        T::deserialize(Value {
            t: self,
            pos: &mut { pos },
        })
    }

    fn str_at(&self, i: u64) -> Result<&str, DecodeError> {
        let &(at, len) = self.strings.get(i as usize).ok_or_else(truncated)?;
        let (at, len) = (at as usize, len as usize);
        std::str::from_utf8(self.bytes.get(at..at + len).ok_or_else(truncated)?).map_err(|e| DecodeError(e.to_string()))
    }

    /// Past the value at `pos`.
    fn skip(&self, pos: &mut usize) -> Result<(), DecodeError> {
        let tag = *self.bytes.get(*pos).ok_or_else(truncated)?;
        *pos += 1;
        match tag {
            T_NULL | T_FALSE | T_TRUE => {}
            T_UINT | T_STR => {
                get_var(&self.bytes, pos).ok_or_else(truncated)?;
            }
            T_INT | T_FLOAT => *pos += 8,
            T_ARRAY => {
                for _ in 0..get_var(&self.bytes, pos).ok_or_else(truncated)? {
                    self.skip(pos)?;
                }
            }
            T_OBJECT => {
                for _ in 0..get_var(&self.bytes, pos).ok_or_else(truncated)? {
                    get_var(&self.bytes, pos).ok_or_else(truncated)?;
                    self.skip(pos)?;
                }
            }
            t => return Err(DecodeError(format!("data.cache: unknown tag {t}"))),
        }
        Ok(())
    }

    /// The root array: where each element starts, nothing decoded. `None` if the root is not an array.
    pub fn array_positions(&self) -> Result<Option<Vec<usize>>, DecodeError> {
        let mut pos = self.root;
        if self.bytes.get(pos) != Some(&T_ARRAY) {
            return Ok(None);
        }
        pos += 1;
        let n = get_var(&self.bytes, &mut pos).ok_or_else(truncated)? as usize;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(pos);
            self.skip(&mut pos)?;
        }
        Ok(Some(out))
    }

    /// The string under `key` of the object at `pos`, without decoding the rest.
    pub fn object_str(&self, pos: usize, key: &str) -> Result<Option<&str>, DecodeError> {
        let mut p = pos;
        if self.bytes.get(p) != Some(&T_OBJECT) {
            return Ok(None);
        }
        p += 1;
        for _ in 0..get_var(&self.bytes, &mut p).ok_or_else(truncated)? {
            let k = get_var(&self.bytes, &mut p).ok_or_else(truncated)?;
            if self.str_at(k)? == key {
                if self.bytes.get(p) != Some(&T_STR) {
                    return Ok(None);
                }
                p += 1;
                let s = get_var(&self.bytes, &mut p).ok_or_else(truncated)?;
                return self.str_at(s).map(Some);
            }
            self.skip(&mut p)?;
        }
        Ok(None)
    }
}

/// A value of a table as a serde deserializer: behaves as `serde_json` does on the same JSON.
struct Value<'a, 'p> {
    t: &'a Table,
    pos: &'p mut usize,
}

impl Value<'_, '_> {
    fn peek(&self) -> Result<u8, DecodeError> {
        self.t.bytes.get(*self.pos).copied().ok_or_else(truncated)
    }
    fn var(&mut self) -> Result<u64, DecodeError> {
        get_var(&self.t.bytes, self.pos).ok_or_else(truncated)
    }
    fn word(&mut self) -> Result<[u8; 8], DecodeError> {
        let b = self.t.bytes.get(*self.pos..*self.pos + 8).ok_or_else(truncated)?;
        *self.pos += 8;
        b.try_into().map_err(|_| truncated())
    }
}

impl<'de> de::Deserializer<'de> for Value<'_, '_> {
    type Error = DecodeError;

    fn deserialize_any<V: Visitor<'de>>(mut self, v: V) -> Result<V::Value, DecodeError> {
        let tag = self.peek()?;
        *self.pos += 1;
        match tag {
            T_NULL => v.visit_unit(),
            T_FALSE => v.visit_bool(false),
            T_TRUE => v.visit_bool(true),
            T_UINT => v.visit_u64(self.var()?),
            T_INT => v.visit_i64(i64::from_le_bytes(self.word()?)),
            T_FLOAT => v.visit_f64(f64::from_le_bytes(self.word()?)),
            T_STR => {
                let i = self.var()?;
                v.visit_str(self.t.str_at(i)?)
            }
            T_ARRAY => {
                let left = self.var()?;
                let r = v.visit_seq(Items {
                    t: self.t,
                    pos: self.pos,
                    left,
                })?;
                Ok(r)
            }
            T_OBJECT => {
                let left = self.var()?;
                v.visit_map(Fields {
                    t: self.t,
                    pos: self.pos,
                    left,
                })
            }
            t => Err(DecodeError(format!("data.cache: unknown tag {t}"))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DecodeError> {
        if self.peek()? == T_NULL {
            *self.pos += 1;
            v.visit_none()
        } else {
            v.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, DecodeError> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        mut self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DecodeError> {
        match self.peek()? {
            T_STR => {
                *self.pos += 1;
                let i = self.var()?;
                v.visit_enum(self.t.str_at(i)?.to_owned().into_deserializer())
            }
            T_OBJECT => {
                *self.pos += 1;
                if self.var()? != 1 {
                    return Err(DecodeError("data.cache: an enum object without exactly one key".to_owned()));
                }
                v.visit_enum(Variant { t: self.t, pos: self.pos })
            }
            _ => Err(DecodeError("data.cache: not an enum".to_owned())),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct Items<'a, 'p> {
    t: &'a Table,
    pos: &'p mut usize,
    left: u64,
}

impl<'de> SeqAccess<'de> for Items<'_, '_> {
    type Error = DecodeError;
    fn next_element_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<Option<S::Value>, DecodeError> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(Value { t: self.t, pos: self.pos }).map(Some)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.left as usize)
    }
}

struct Fields<'a, 'p> {
    t: &'a Table,
    pos: &'p mut usize,
    left: u64,
}

impl<'de> MapAccess<'de> for Fields<'_, '_> {
    type Error = DecodeError;
    fn next_key_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<Option<S::Value>, DecodeError> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        let i = get_var(&self.t.bytes, self.pos).ok_or_else(truncated)?;
        seed.deserialize(Key(self.t.str_at(i)?)).map(Some)
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, DecodeError> {
        seed.deserialize(Value { t: self.t, pos: self.pos })
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.left as usize)
    }
}

/// An object key, as `serde_json` serves it: a string, or a number or bool parsed from it when the target asks for one.
struct Key<'a>(&'a str);

macro_rules! key_number {
    ($($de:ident $visit:ident)*) => {$(
        fn $de<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DecodeError> {
            match self.0.parse() {
                Ok(n) => v.$visit(n),
                Err(_) => Err(DecodeError(format!("data.cache: the key '{}' is not a number", self.0))),
            }
        }
    )*};
}

impl<'de> de::Deserializer<'de> for Key<'_> {
    type Error = DecodeError;
    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DecodeError> {
        v.visit_str(self.0)
    }
    key_number! {
        deserialize_i8 visit_i8 deserialize_i16 visit_i16 deserialize_i32 visit_i32 deserialize_i64 visit_i64
        deserialize_u8 visit_u8 deserialize_u16 visit_u16 deserialize_u32 visit_u32 deserialize_u64 visit_u64
        deserialize_f32 visit_f32 deserialize_f64 visit_f64 deserialize_bool visit_bool
    }
    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DecodeError> {
        v.visit_some(self)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, DecodeError> {
        v.visit_newtype_struct(self)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DecodeError> {
        v.visit_enum(self.0.to_owned().into_deserializer())
    }
    serde::forward_to_deserialize_any! {
        i128 u128 char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

/// `{"variant": value}`.
struct Variant<'a, 'p> {
    t: &'a Table,
    pos: &'p mut usize,
}

impl<'de, 'a, 'p> EnumAccess<'de> for Variant<'a, 'p> {
    type Error = DecodeError;
    type Variant = Self;
    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, Self), DecodeError> {
        let i = get_var(&self.t.bytes, self.pos).ok_or_else(truncated)?;
        let name = seed.deserialize(Key(self.t.str_at(i)?))?;
        Ok((name, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, '_> {
    type Error = DecodeError;
    fn unit_variant(self) -> Result<(), DecodeError> {
        de::Deserialize::deserialize(Value { t: self.t, pos: self.pos })
    }
    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, DecodeError> {
        seed.deserialize(Value { t: self.t, pos: self.pos })
    }
    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, v: V) -> Result<V::Value, DecodeError> {
        de::Deserializer::deserialize_any(Value { t: self.t, pos: self.pos }, v)
    }
    fn struct_variant<V: Visitor<'de>>(self, _fields: &'static [&'static str], v: V) -> Result<V::Value, DecodeError> {
        de::Deserializer::deserialize_any(Value { t: self.t, pos: self.pos }, v)
    }
}
