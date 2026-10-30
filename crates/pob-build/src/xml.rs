//! PoB's build-XML dialect -- elements/attributes/text, entities, CDATA, comments, a leading `<?xml?>` -- as pob-ts
//! `src/build/xml.ts` reads it (checked against the oracle's `xml.ParseXML`, D22). One linear pass over the bytes; every
//! delimiter is ASCII, so slices of the UTF-8 input stay UTF-8 (L16: text is bytes, nothing is re-encoded).
//!
//! `runtime/lua/xml.lua` semantics kept: `decodeContent` (:9-15) = `gsub("&(.-);", ent -> entTbl[ent] or "")` -- only the
//! five named entities survive, every other `&...;` (numeric references, unknown names) becomes "", the lazy `.-` runs to
//! the FIRST `;` over any byte, an `&` with no later `;` stays; applied per attribute value (:28) and per text run (:46);
//! comments are stripped before parsing (:36), so a text run continues across a comment; CDATA (:41-50) is literal and ends
//! a run. Attributes: a repeated name keeps the last value (Lua table write), first-seen position.
//! As pob-ts (L19): `\r\n`/`\r` -> `\n` up front, a UTF-8 BOM is dropped.

use std::fmt;

use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct XmlElement {
    pub name: String,
    pub attrib: IndexMap<String, String>,
    pub children: Vec<XmlNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum XmlNode {
    Element(XmlElement),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    pub message: String,
    /// 1-based line and column (characters) of the failure.
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}:{}", self.message, self.line, self.column)
    }
}

impl std::error::Error for XmlError {}

impl XmlElement {
    /// The element children, text dropped.
    pub fn child_elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|c| match c {
            XmlNode::Element(e) => Some(e),
            XmlNode::Text(_) => None,
        })
    }

    /// The text children concatenated in order, exactly as written (no trim).
    pub fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            if let XmlNode::Text(t) = c {
                out.push_str(t);
            }
        }
        out
    }

    /// `attrib.<name>`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrib.get(name).map(String::as_str)
    }
}

/// xml.lua:9-15 `decodeContent`.
pub fn decode_content(content: &str) -> String {
    let b = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'&' {
            i += 1;
            continue;
        }
        let Some(rel) = b[i + 1..].iter().position(|&c| c == b';') else {
            break; // no `;` after this `&`: neither it nor any later `&` can match
        };
        let semi = i + 1 + rel;
        out.push_str(&content[last..i]);
        out.push_str(match &content[i + 1..semi] {
            "lt" => "<",
            "gt" => ">",
            "amp" => "&",
            "apos" => "'",
            "quot" => "\"",
            _ => "",
        });
        i = semi + 1;
        last = i;
    }
    out.push_str(&content[last..]);
    out
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b':'
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-')
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn fail<T>(&self, message: String, at: usize) -> Result<T, XmlError> {
        let (mut line, mut column) = (1, 1);
        for ch in self.s[..at.min(self.s.len())].chars() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += ch.len_utf16();
            }
        }
        Err(XmlError { message, line, column })
    }

    fn starts_with(&self, p: &str) -> bool {
        self.b[self.i..].starts_with(p.as_bytes())
    }

    fn find(&self, p: &str, from: usize) -> Option<usize> {
        let from = from.min(self.b.len());
        self.s[from..].find(p).map(|k| from + k)
    }

    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    /// Comments and `<?...?>` wherever they may appear.
    fn skip_misc(&mut self) -> Result<bool, XmlError> {
        if self.starts_with("<!--") {
            let Some(e) = self.find("-->", self.i + 4) else {
                return self.fail("unterminated comment".into(), self.i);
            };
            self.i = e + 3;
            return Ok(true);
        }
        if self.starts_with("<?") {
            let Some(e) = self.find("?>", self.i + 2) else {
                return self.fail("unterminated processing instruction".into(), self.i);
            };
            self.i = e + 2;
            return Ok(true);
        }
        Ok(false)
    }

    fn parse_name(&mut self) -> Result<&'a str, XmlError> {
        if self.i >= self.b.len() || !is_name_start(self.b[self.i]) {
            return self.fail("expected a name".into(), self.i);
        }
        let start = self.i;
        self.i += 1;
        while self.i < self.b.len() && is_name_char(self.b[self.i]) {
            self.i += 1;
        }
        Ok(&self.s[start..self.i])
    }

    fn parse_attr_value(&mut self) -> Result<String, XmlError> {
        let quote = self.b.get(self.i).copied();
        let q = match quote {
            Some(b'"') => "\"",
            Some(b'\'') => "'",
            _ => return self.fail("expected an attribute value".into(), self.i),
        };
        self.i += 1;
        let Some(e) = self.find(q, self.i) else {
            return self.fail("unterminated attribute value".into(), self.i);
        };
        let raw = &self.s[self.i..e];
        self.i = e + 1;
        Ok(decode_content(raw))
    }

    fn parse_attributes(&mut self) -> Result<IndexMap<String, String>, XmlError> {
        let mut attrib = IndexMap::new();
        loop {
            self.skip_ws();
            if self.i >= self.b.len() || matches!(self.b[self.i], b'>' | b'/') {
                return Ok(attrib);
            }
            let name = self.parse_name()?;
            self.skip_ws();
            if self.b.get(self.i) != Some(&b'=') {
                return self.fail(format!("expected '=' after attribute '{name}'"), self.i);
            }
            self.i += 1;
            self.skip_ws();
            let v = self.parse_attr_value()?;
            attrib.insert(name.to_owned(), v);
        }
    }

    fn parse_cdata(&mut self) -> Result<&'a str, XmlError> {
        let Some(e) = self.find("]]>", self.i + 9) else {
            return self.fail("unterminated CDATA section".into(), self.i);
        };
        let content = &self.s[self.i + 9..e];
        self.i = e + 3;
        Ok(content)
    }

    fn parse_element(&mut self) -> Result<XmlElement, XmlError> {
        self.i += 1; // '<'
        let name = self.parse_name()?.to_owned();
        self.skip_ws();
        let attrib = self.parse_attributes()?;
        if self.starts_with("/>") {
            self.i += 2;
            return Ok(XmlElement {
                name,
                attrib,
                children: Vec::new(),
            });
        }
        if self.b.get(self.i) != Some(&b'>') {
            return self.fail(format!("expected '>' or '/>' for '<{name}'"), self.i);
        }
        self.i += 1;
        let mut children = Vec::new();
        // `buf` = decoded text so far, `raw` = the pending undecoded run (continues across comments, ends at CDATA).
        let mut buf = String::new();
        let mut raw = String::new();
        loop {
            if self.i >= self.b.len() {
                return self.fail(format!("unterminated element '{name}'"), self.i);
            }
            if self.skip_misc()? {
                continue;
            }
            if self.starts_with("<![CDATA[") {
                buf.push_str(&decode_content(&raw));
                raw.clear();
                let c = self.parse_cdata()?;
                buf.push_str(c);
                continue;
            }
            if self.starts_with("</") {
                buf.push_str(&decode_content(&raw));
                if !buf.is_empty() {
                    children.push(XmlNode::Text(std::mem::take(&mut buf)));
                }
                self.i += 2;
                let close = self.parse_name()?;
                self.skip_ws();
                if self.b.get(self.i) != Some(&b'>') {
                    return self.fail(format!("expected '>' after '</{close}'"), self.i);
                }
                self.i += 1;
                if close != name {
                    return self.fail(format!("mismatched closing tag: expected '</{name}>', got '</{close}>'"), self.i);
                }
                return Ok(XmlElement { name, attrib, children });
            }
            if self.b[self.i] == b'<' {
                buf.push_str(&decode_content(&raw));
                raw.clear();
                if !buf.is_empty() {
                    children.push(XmlNode::Text(std::mem::take(&mut buf)));
                }
                children.push(XmlNode::Element(self.parse_element()?));
                continue;
            }
            // Plain text up to the next '<' (a whole UTF-8 run at once).
            let end = self.b[self.i..]
                .iter()
                .position(|&c| c == b'<')
                .map_or(self.b.len(), |k| self.i + k);
            raw.push_str(&self.s[self.i..end]);
            self.i = end;
        }
    }
}

/// Parse a build XML into its root element.
pub fn parse_xml(raw_text: &str) -> Result<XmlElement, XmlError> {
    let no_crlf = raw_text.replace("\r\n", "\n").replace('\r', "\n");
    let text = no_crlf.strip_prefix('\u{feff}').unwrap_or(&no_crlf);
    let mut p = Parser {
        s: text,
        b: text.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    while p.skip_misc()? {
        p.skip_ws();
    }
    if p.i >= p.b.len() || p.b[p.i] != b'<' {
        return p.fail("expected the root element".into(), p.i);
    }
    p.parse_element()
}
