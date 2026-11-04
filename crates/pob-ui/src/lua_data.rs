//! A reader of the Lua data files of `TreeData/` (`tree.lua`, `sprites.lua`, `3_19/Assets.lua`, `legion/tree-legion.lua`): one
//! `return { ... }` of a table constructor of constants. UI-only fields (`sprites`, `assets`, `activeIcon`, group `orbits`, ...) are not
//! in `data/tree/*.json` where the calc reads them, and the SOURCE ORDER of the keys decides the `pairs` order (the constructor fills the
//! table in that order, MINEFIELD D13), so the source is read as it is.

use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum LuaKey {
    Num(f64),
    Str(Rc<str>),
}

#[derive(Clone, Debug)]
pub enum LuaVal {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Table(Rc<LuaTab>),
}

/// A constructor: positional items (`arr`, Lua index = position + 1) and keyed fields (`[k]=v`, `k=v`) in source order.
#[derive(Clone, Debug, Default)]
pub struct LuaTab {
    pub arr: Vec<LuaVal>,
    pub fields: Vec<(LuaKey, LuaVal)>,
}

impl LuaTab {
    pub fn get(&self, key: &str) -> Option<&LuaVal> {
        self.fields.iter().find_map(|(k, v)| match k {
            LuaKey::Str(s) if &**s == key => Some(v),
            _ => None,
        })
    }

    pub fn tab(&self, key: &str) -> Option<&Rc<LuaTab>> {
        self.get(key).and_then(LuaVal::as_tab)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(LuaVal::as_str)
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(LuaVal::as_num)
    }

    /// `t[n]` with a number key: a `[n]=` field, or the positional item for an integral `n >= 1`.
    pub fn get_num(&self, key: f64) -> Option<&LuaVal> {
        if key >= 1.0 && key.fract() == 0.0 && (key as usize) <= self.arr.len() {
            return Some(&self.arr[key as usize - 1]);
        }
        self.fields.iter().find_map(|(k, v)| match k {
            LuaKey::Num(n) if *n == key => Some(v),
            _ => None,
        })
    }

    /// `#t` of a list written positionally or as `[1]=`, `[2]=`, ... .
    pub fn len_seq(&self) -> usize {
        let mut n = self.arr.len();
        while self.get_num((n + 1) as f64).is_some() {
            n += 1;
        }
        n
    }

    /// `ipairs(t)` values.
    pub fn seq(&self) -> Vec<&LuaVal> {
        (1..=self.len_seq()).filter_map(|i| self.get_num(i as f64)).collect()
    }
}

impl LuaVal {
    pub fn as_tab(&self) -> Option<&Rc<LuaTab>> {
        if let LuaVal::Table(t) = self { Some(t) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let LuaVal::Str(s) = self { Some(s) } else { None }
    }

    pub fn as_num(&self) -> Option<f64> {
        if let LuaVal::Num(n) = self { Some(*n) } else { None }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("{what} at byte {}", self.i)
    }

    fn skip_ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"--") {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                return;
            }
        }
    }

    fn string(&mut self) -> Result<Rc<str>, String> {
        let q = self.s[self.i];
        self.i += 1;
        let start = self.i;
        // the fast path: no escapes
        while self.i < self.s.len() && self.s[self.i] != q && self.s[self.i] != b'\\' {
            self.i += 1;
        }
        if self.i < self.s.len() && self.s[self.i] == q {
            let r: Rc<str> = String::from_utf8_lossy(&self.s[start..self.i]).into();
            self.i += 1;
            return Ok(r);
        }
        let mut out = self.s[start..self.i].to_vec();
        while self.i < self.s.len() && self.s[self.i] != q {
            let c = self.s[self.i];
            self.i += 1;
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let e = *self.s.get(self.i).ok_or_else(|| self.err("bad escape"))?;
            self.i += 1;
            match e {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'0'..=b'9' => {
                    let mut v = u32::from(e - b'0');
                    for _ in 0..2 {
                        match self.s.get(self.i) {
                            Some(d) if d.is_ascii_digit() => {
                                v = v * 10 + u32::from(d - b'0');
                                self.i += 1;
                            }
                            _ => break,
                        }
                    }
                    out.push(v as u8);
                }
                other => out.push(other),
            }
        }
        self.i += 1;
        Ok(String::from_utf8_lossy(&out).into())
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.i;
        while self.i < self.s.len()
            && matches!(self.s[self.i], b'0'..=b'9' | b'.' | b'-' | b'+' | b'e' | b'E' | b'x' | b'a'..=b'd' | b'f' | b'A'..=b'D' | b'F')
        {
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?;
        t.parse::<f64>().map_err(|_| self.err(&format!("bad number {t:?}")))
    }

    fn value(&mut self) -> Result<LuaVal, String> {
        self.skip_ws();
        match self.s.get(self.i) {
            Some(b'{') => self.table(),
            Some(b'"' | b'\'') => Ok(LuaVal::Str(self.string()?)),
            Some(c) if c.is_ascii_digit() || *c == b'-' || *c == b'.' => Ok(LuaVal::Num(self.number()?)),
            _ => {
                for (w, v) in [("true", LuaVal::Bool(true)), ("false", LuaVal::Bool(false)), ("nil", LuaVal::Nil)] {
                    if self.s[self.i..].starts_with(w.as_bytes()) {
                        self.i += w.len();
                        return Ok(v);
                    }
                }
                Err(self.err("unexpected token"))
            }
        }
    }

    fn table(&mut self) -> Result<LuaVal, String> {
        self.i += 1;
        let mut t = LuaTab::default();
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return Err(self.err("unterminated table")),
                Some(b'}') => {
                    self.i += 1;
                    return Ok(LuaVal::Table(Rc::new(t)));
                }
                Some(b',' | b';') => self.i += 1,
                Some(b'[') => {
                    self.i += 1;
                    self.skip_ws();
                    let key = match self.value()? {
                        LuaVal::Num(n) => LuaKey::Num(n),
                        LuaVal::Str(s) => LuaKey::Str(s),
                        _ => return Err(self.err("bad key")),
                    };
                    self.skip_ws();
                    if self.s.get(self.i) != Some(&b']') {
                        return Err(self.err("expected ]"));
                    }
                    self.i += 1;
                    self.skip_ws();
                    if self.s.get(self.i) != Some(&b'=') {
                        return Err(self.err("expected ="));
                    }
                    self.i += 1;
                    let v = self.value()?;
                    t.fields.push((key, v));
                }
                Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {
                    let start = self.i;
                    while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                        self.i += 1;
                    }
                    let word = &self.s[start..self.i];
                    let after = self.i;
                    self.skip_ws();
                    if self.s.get(self.i) == Some(&b'=') {
                        self.i += 1;
                        let k: Rc<str> = String::from_utf8_lossy(word).into();
                        let v = self.value()?;
                        t.fields.push((LuaKey::Str(k), v));
                    } else {
                        self.i = start;
                        let _ = after;
                        let v = self.value()?;
                        t.arr.push(v);
                    }
                }
                Some(_) => {
                    let v = self.value()?;
                    t.arr.push(v);
                }
            }
        }
    }
}

/// `dofile` of a data file: the value of its `return`.
pub fn parse_return(src: &[u8]) -> Result<LuaVal, String> {
    let at = src.windows(6).position(|w| w == b"return").ok_or("no return")?;
    let mut p = Parser { s: src, i: at + 6 };
    p.skip_ws();
    // `local tree={ ... }return tree` (the first word after `return` is a name, and `return` itself came after the table)
    let name_end = src[p.i..]
        .iter()
        .position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
        .map_or(src.len(), |n| p.i + n);
    let name = &src[p.i..name_end];
    if name.first().is_some_and(u8::is_ascii_alphabetic) && !matches!(name, b"true" | b"false" | b"nil") {
        let local = [b"local ", name, b"="].concat();
        let start = src
            .windows(local.len())
            .position(|w| w == local)
            .ok_or("return of an unknown name")?;
        p.i = start + local.len();
    }
    p.value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shapes_of_tree_data() {
        let v = parse_return(b"-- c\nreturn { [\"a\"]= { 1, 2.5, -3 }, [7]= \"x\\\"y\", b = true, [0.5]= {x=1}, }").unwrap();
        let t = v.as_tab().unwrap();
        assert_eq!(t.tab("a").unwrap().arr.len(), 3);
        assert_eq!(t.tab("a").unwrap().arr[2].as_num(), Some(-3.0));
        assert_eq!(t.get_num(7.0).unwrap().as_str(), Some("x\"y"));
        assert!(matches!(t.get("b"), Some(LuaVal::Bool(true))));
        assert_eq!(t.get_num(0.5).unwrap().as_tab().unwrap().num("x"), Some(1.0));
        assert_eq!(t.fields.len(), 4);
    }

    #[test]
    fn reads_a_table_returned_by_name() {
        let v = parse_return(b"local tree={[\"a\"]=1}return tree").unwrap();
        assert_eq!(v.as_tab().unwrap().num("a"), Some(1.0));
        assert!(parse_return(b"local x={}return tree").is_err());
    }
}
