//! Reader of the stream `lua/dump.lua` writes (grammar in its header): one arena of tables and functions, shared and cyclic
//! values by id, exactly as they were in the Lua state.

use std::collections::HashMap;

/// A Lua value. Tables and functions are arena ids (the dump's own ids).
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Box<str>),
    Table(u32),
    Func(u32),
    /// userdata / thread / cdata / an unwalked table (`_G`): its description.
    Opaque(Box<str>),
}

impl V {
    pub fn as_str(&self) -> Option<&str> {
        if let V::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_num(&self) -> Option<f64> {
        if let V::Num(n) = self { Some(*n) } else { None }
    }
    pub fn as_table(&self) -> Option<u32> {
        if let V::Table(t) = self { Some(*t) } else { None }
    }
}

#[derive(Debug, Default)]
pub struct Table {
    /// In the dump's key order: numbers ascending, strings in byte order, booleans, table keys by id.
    pub entries: Vec<(V, V)>,
    by_str: HashMap<Box<str>, usize>,
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&V> {
        self.by_str.get(key).map(|&i| &self.entries[i].1)
    }
    /// `t[i]` for an integral key.
    pub fn index(&self, i: i64) -> Option<&V> {
        // Number keys come first, ascending: binary search them.
        let nums = self.entries.partition_point(|(k, _)| matches!(k, V::Num(_)));
        self.entries[..nums]
            .binary_search_by(|(k, _)| k.as_num().unwrap().total_cmp(&(i as f64)))
            .ok()
            .map(|p| &self.entries[p].1)
    }
    /// Keys are exactly the integers 1..n (n >= 0): a Lua sequence.
    pub fn is_sequence(&self) -> bool {
        self.entries.iter().enumerate().all(|(i, (k, _))| *k == V::Num((i + 1) as f64))
    }
}

#[derive(Debug)]
pub enum Func {
    C,
    Lua { src: Box<str>, from: u32, to: u32, upvalues: Vec<(Box<str>, V)> },
}

#[derive(Default)]
pub struct Dump {
    pub tables: Vec<Table>,
    pub funcs: Vec<Func>,
    /// dump id -> (is_table, index into tables/funcs)
    ids: Vec<(bool, u32)>,
    pub roots: Vec<(String, V)>,
    /// Top-level keys of each root some converter asked for (the coverage report: a new `data.*` module shows up).
    pub touched: std::cell::RefCell<std::collections::BTreeSet<(String, String)>>,
}

impl Dump {
    /// Top-level keys of `root` no converter read.
    pub fn untouched(&self, root: &str) -> Vec<String> {
        let touched = self.touched.borrow();
        let Some(t) = self.root(root).as_table() else { return Vec::new() };
        self.t(t)
            .entries
            .iter()
            .filter_map(|(k, _)| k.as_str())
            .filter(|k| !touched.contains(&(root.to_owned(), (*k).to_owned())))
            .map(str::to_owned)
            .collect()
    }
    pub fn t(&self, id: u32) -> &Table {
        &self.tables[id as usize]
    }
    pub fn f(&self, id: u32) -> &Func {
        &self.funcs[id as usize]
    }
    pub fn root(&self, name: &str) -> &V {
        &self.roots.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("dump: no root '{name}'")).1
    }
    /// `root.a.b.c` through string keys.
    pub fn path(&self, root: &str, keys: &[&str]) -> Option<&V> {
        let mut v = self.root(root);
        if let Some(k) = keys.first() {
            self.touched.borrow_mut().insert((root.to_owned(), (*k).to_owned()));
        }
        for k in keys {
            v = self.t(v.as_table()?).get(k)?;
        }
        Some(v)
    }

    pub fn parse(bytes: &[u8]) -> Result<Dump, String> {
        let mut p = Parser { b: bytes, i: 0, d: Dump::default() };
        p.d.ids.push((false, u32::MAX)); // ids start at 1
        while p.i < p.b.len() {
            let name = match p.value()? {
                V::Str(s) => s.into_string(),
                v => return Err(format!("dump: root name expected, got {v:?}")),
            };
            let v = p.value()?;
            p.d.roots.push((name, v));
        }
        Ok(p.d)
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    d: Dump,
}

impl Parser<'_> {
    fn err<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("dump: {m} at byte {}", self.i))
    }
    fn byte(&mut self) -> Result<u8, String> {
        let c = *self.b.get(self.i).ok_or("dump: unexpected end")?;
        self.i += 1;
        Ok(c)
    }
    fn until(&mut self, end: u8) -> Result<&str, String> {
        let start = self.i;
        while *self.b.get(self.i).ok_or("dump: unexpected end")? != end {
            self.i += 1;
        }
        self.i += 1;
        std::str::from_utf8(&self.b[start..self.i - 1]).map_err(|e| e.to_string())
    }
    fn int(&mut self) -> Result<u32, String> {
        self.until(b';')?.parse().map_err(|e| format!("dump: bad integer: {e}"))
    }
    fn string(&mut self) -> Result<Box<str>, String> {
        let len: usize = self.until(b':')?.parse().map_err(|e| format!("dump: bad length: {e}"))?;
        let s = &self.b[self.i..self.i + len];
        self.i += len;
        // PoB's data is UTF-8 throughout; anything else is reported, not guessed.
        std::str::from_utf8(s).map(Into::into).map_err(|_| format!("dump: invalid UTF-8 string {:?}", String::from_utf8_lossy(s)))
    }
    fn register(&mut self, id: u32, is_table: bool, idx: u32) -> Result<(), String> {
        // Ids are assigned by a separate pass, so definitions arrive in any order -- but never twice.
        let id = id as usize;
        if id >= self.d.ids.len() {
            self.d.ids.resize(id + 1, (false, u32::MAX));
        }
        if self.d.ids[id].1 != u32::MAX {
            return self.err(&format!("id {id} defined twice"));
        }
        self.d.ids[id] = (is_table, idx);
        Ok(())
    }
    fn value(&mut self) -> Result<V, String> {
        Ok(match self.byte()? {
            b'N' => V::Nil,
            b'T' => V::Bool(true),
            b'F' => V::Bool(false),
            b'#' => V::Num(match self.until(b';')? {
                "nan" => f64::NAN,
                "inf" => f64::INFINITY,
                "-inf" => f64::NEG_INFINITY,
                "-0" => -0.0,
                t => t.parse().map_err(|e| format!("dump: bad number {t}: {e}"))?,
            }),
            b's' => V::Str(self.string()?),
            b'{' => {
                let id = self.int()?;
                let idx = self.d.tables.len() as u32;
                self.register(id, true, idx)?;
                self.d.tables.push(Table::default());
                let mut entries = Vec::new();
                while self.b.get(self.i) != Some(&b'}') {
                    let k = self.value()?;
                    let v = self.value()?;
                    entries.push((k, v));
                }
                self.i += 1;
                let t = &mut self.d.tables[idx as usize];
                for (i, (k, _)) in entries.iter().enumerate() {
                    if let V::Str(s) = k {
                        t.by_str.insert(s.clone(), i);
                    }
                }
                t.entries = entries;
                V::Table(idx)
            }
            b'@' => {
                let id = self.int()?;
                let &(is_table, idx) = self.d.ids.get(id as usize).filter(|e| e.1 != u32::MAX).ok_or("dump: ref before its definition")?;
                if is_table { V::Table(idx) } else { V::Func(idx) }
            }
            b'C' => {
                let id = self.int()?;
                let idx = self.d.funcs.len() as u32;
                self.register(id, false, idx)?;
                self.d.funcs.push(Func::C);
                V::Func(idx)
            }
            b'L' => {
                let id = self.int()?;
                let idx = self.d.funcs.len() as u32;
                self.register(id, false, idx)?;
                self.d.funcs.push(Func::C); // placeholder until the upvalues are read (they may refer back to it)
                if self.byte()? != b's' {
                    return self.err("function source expected");
                }
                let src = self.string()?;
                let from = self.int()?;
                let to = self.int()?;
                let n = self.int()?;
                let mut upvalues = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    if self.byte()? != b's' {
                        return self.err("upvalue name expected");
                    }
                    let name = self.string()?;
                    upvalues.push((name, self.value()?));
                }
                self.d.funcs[idx as usize] = Func::Lua { src, from, to, upvalues };
                V::Func(idx)
            }
            b'U' => {
                if self.byte()? != b's' {
                    return self.err("opaque description expected");
                }
                V::Opaque(self.string()?)
            }
            c => return self.err(&format!("unexpected tag {:?}", c as char)),
        })
    }
}
