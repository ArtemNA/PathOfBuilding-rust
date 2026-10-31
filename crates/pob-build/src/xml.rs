//! `runtime/lua/xml.lua` `ParseXML` (:35-96), ported verbatim -- the same Lua patterns run through `pob_lua::pattern`
//! (MINEFIELD L13), on bytes (L16: the input may be any byte string; nothing is decoded or re-encoded).
//!
//! What that means (R3.8a corpus, `tests/fixtures/xml-lua.json`): comments are stripped first (`<!%-%-.-%-%->`, so a text
//! run continues across one); every text run is trimmed of ASCII whitespace and dropped when only whitespace remains; the
//! result is the top-level list (text before/after the root and several roots included); `<?...?>` anywhere is skipped;
//! `<!DOCTYPE`, `< a>`, ... end the tag scan and the rest becomes text; attributes are `(%w+)=(["'])(.-)%2` (no spaces
//! around `=`, `a-b="v"` reads as `b`, a repeated key keeps the last value -- Lua table write, first-seen position here);
//! entities are `decodeContent` (D22: only the five named ones survive, every other `&...;` becomes "" -- the lazy `.-`
//! runs to the FIRST `;`); CDATA is literal and only kept when it has a non-space byte. No CRLF or BOM handling: a BOM is
//! a text node before the root, as in Lua. Errors are the `nil, msg` strings of xml.lua.

use std::fmt;

use indexmap::IndexMap;
use pob_lua::{Capture, Captures, PatternError, Repl, gmatch, gsub, match_};

/// One element of the tree as UTF-8 strings (see [`parse_xml_document`]).
#[derive(Clone, Debug, PartialEq)]
pub struct XmlElement {
    pub name: String,
    pub attrib: IndexMap<String, String>,
    pub children: Vec<XmlNode>,
    /// xml.lua's `empty` flag: set on `<a/>` (:61) and on a closed element with no children (:77-79).
    pub empty: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum XmlNode {
    Element(XmlElement),
    Text(String),
}

/// One element of the tree as byte strings (see [`parse_xml_bytes`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RawXmlElement {
    pub name: Vec<u8>,
    pub attrib: IndexMap<Vec<u8>, Vec<u8>>,
    pub children: Vec<RawXmlNode>,
    pub empty: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RawXmlNode {
    Element(RawXmlElement),
    Text(Vec<u8>),
}

/// A failed `ParseXML`: `thrown == false` is the `nil, errMsg` return (the message is xml.lua's text);
/// `thrown == true` is a Lua error raised by the pattern engine (e.g. "pattern too complex").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    pub message: String,
    pub thrown: bool,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for XmlError {}

impl From<PatternError> for XmlError {
    fn from(e: PatternError) -> Self {
        XmlError {
            message: e.0,
            thrown: true,
        }
    }
}

fn err<T>(message: String) -> Result<T, XmlError> {
    Err(XmlError { message, thrown: false })
}

impl XmlElement {
    /// The element children, text dropped (`type(node) == "table"`).
    pub fn child_elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|c| match c {
            XmlNode::Element(e) => Some(e),
            XmlNode::Text(_) => None,
        })
    }

    /// The text children concatenated in order.
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

/// LuaJIT `lj_char_isspace` (C locale): `%s`.
fn is_space(c: u8) -> bool {
    c == b' ' || (b'\t'..=b'\r').contains(&c)
}

/// `s:find("%S")`.
fn has_non_space(s: &[u8]) -> bool {
    s.iter().any(|&c| !is_space(c))
}

fn cap<'a>(s: &'a [u8], caps: &Captures, i: usize) -> &'a [u8] {
    match caps.get(i) {
        Some(Capture::Str { start, end }) => &s[start..end],
        other => unreachable!("capture {i} is a string capture, got {other:?}"),
    }
}

fn pos(caps: &Captures, i: usize) -> usize {
    match caps.get(i) {
        Some(Capture::Pos(p)) => p,
        other => unreachable!("capture {i} is a position capture, got {other:?}"),
    }
}

/// xml.lua:9-15 `decodeContent`: `content:gsub("&(.-);", entTbl[ent] or "")`.
pub fn decode_content(content: &[u8]) -> Vec<u8> {
    if !content.contains(&b'&') {
        return content.to_vec(); // gsub finds no match: the subject unchanged
    }
    let mut ent = |subject: &[u8], caps: &Captures| -> Option<Vec<u8>> {
        let rep: &[u8] = match cap(subject, caps, 0) {
            b"lt" => b"<",
            b"gt" => b">",
            b"amp" => b"&",
            b"apos" => b"'",
            b"quot" => b"\"",
            _ => b"",
        };
        Some(rep.to_vec())
    };
    gsub(content, b"&(.-);", Repl::Fn(&mut ent), None).expect("decodeContent pattern").0
}

/// xml.lua:25-31 `parseAttribs`: `str:gsub("(%w+)=([\"'])(.-)%2", ...)` -- `attrib[key] = decodeContent(val)`.
fn parse_attribs(s: &[u8]) -> Result<IndexMap<Vec<u8>, Vec<u8>>, XmlError> {
    let mut attrib = IndexMap::new();
    for caps in gmatch(s, br#"(%w+)=(["'])(.-)%2"#) {
        let caps = caps?;
        attrib.insert(cap(s, &caps, 0).to_vec(), decode_content(cap(s, &caps, 2)));
    }
    Ok(attrib)
}

/// An open element on xml.lua's `stack` (the bottom one is the nameless `top` list).
struct Frame {
    name: Vec<u8>,
    attrib: IndexMap<Vec<u8>, Vec<u8>>,
    children: Vec<RawXmlNode>,
}

const CDATA_PAT: &[u8] = b"^%s*([^<]-)%s*<!%[CDATA%[(.-)%]%]>()";
const TAG_PAT: &[u8] = b"^%s*([^<]-)%s*<([%?/]?)([%w:]+)(.-)([%?/]?)>()";

/// xml.lua:35-96 `xml.ParseXML(str)` on bytes: the top-level list.
pub fn parse_xml_bytes(input: &[u8]) -> Result<Vec<RawXmlNode>, XmlError> {
    let s = gsub(input, b"<!%-%-.-%-%->", Repl::Str(b""), None)?.0; // :36 strip comments
    let s = s.as_slice();
    let mut stack: Vec<Frame> = vec![Frame {
        name: Vec::new(),
        attrib: IndexMap::new(),
        children: Vec::new(),
    }];
    let mut i = 1usize;
    loop {
        let top = stack.last_mut().expect("the bottom frame is never popped");
        let ie;
        if let Some(caps) = match_(s, CDATA_PAT, Some(i as i64))? {
            // :43-50 CDATA section
            let (content, cdata) = (cap(s, &caps, 0), cap(s, &caps, 1));
            if has_non_space(content) {
                top.children.push(RawXmlNode::Text(decode_content(content)));
            }
            if has_non_space(cdata) {
                top.children.push(RawXmlNode::Text(cdata.to_vec()));
            }
            ie = pos(&caps, 2);
        } else {
            let Some(caps) = match_(s, TAG_PAT, Some(i as i64))? else {
                break; // :54-57 no more tags
            };
            let (content, cs, elem, attrib, ce) = (
                cap(s, &caps, 0),
                cap(s, &caps, 1),
                cap(s, &caps, 2),
                cap(s, &caps, 3),
                cap(s, &caps, 4),
            );
            ie = pos(&caps, 5);
            if has_non_space(content) {
                top.children.push(RawXmlNode::Text(decode_content(content)));
            }
            if cs == b"?" || ce == b"?" {
                // :61 processing instruction, ignored
            } else if ce == b"/" {
                top.children.push(RawXmlNode::Element(RawXmlElement {
                    name: elem.to_vec(),
                    attrib: parse_attribs(attrib)?,
                    children: Vec::new(),
                    empty: true,
                }));
            } else if cs.is_empty() {
                let frame = Frame {
                    name: elem.to_vec(),
                    attrib: parse_attribs(attrib)?,
                    children: Vec::new(),
                };
                stack.push(frame);
            } else {
                // :71-83 end tag
                let elem_s = String::from_utf8_lossy(elem);
                if stack.len() == 1 {
                    return err(format!("nothing to close with '{elem_s}'"));
                }
                let closed = stack.pop().expect("len > 1");
                if closed.name != elem {
                    return err(format!(
                        "trying to close <{}> with '{elem_s}'",
                        String::from_utf8_lossy(&closed.name)
                    ));
                }
                let empty = closed.children.is_empty();
                stack.last_mut().expect("len > 1").children.push(RawXmlNode::Element(RawXmlElement {
                    name: closed.name,
                    attrib: closed.attrib,
                    children: closed.children,
                    empty,
                }));
            }
        }
        i = ie;
    }
    if stack.len() > 1 {
        let name = &stack.last().expect("len > 1").name;
        return err(format!("unclosed element '{}'", String::from_utf8_lossy(name)));
    }
    // :89-92 trailing text
    let caps = match_(s, b"^%s*(.-)%s*$", Some(i as i64))?.expect("`^%s*(.-)%s*$` always matches");
    let content = cap(s, &caps, 0);
    let mut bottom = stack.pop().expect("the bottom frame");
    if has_non_space(content) {
        bottom.children.push(RawXmlNode::Text(decode_content(content)));
    }
    Ok(bottom.children)
}

fn utf8(v: Vec<u8>) -> String {
    String::from_utf8(v).expect("ASCII-delimited slices of a UTF-8 input stay UTF-8")
}

fn to_utf8_node(n: RawXmlNode) -> XmlNode {
    match n {
        RawXmlNode::Text(t) => XmlNode::Text(utf8(t)),
        RawXmlNode::Element(e) => XmlNode::Element(XmlElement {
            name: utf8(e.name),
            attrib: e.attrib.into_iter().map(|(k, v)| (utf8(k), utf8(v))).collect(),
            children: e.children.into_iter().map(to_utf8_node).collect(),
            empty: e.empty,
        }),
    }
}

/// `xml.ParseXML(str)` of a UTF-8 text: every delimiter, trimmed byte and removed entity is ASCII, so all pieces stay UTF-8.
pub fn parse_xml_document(text: &str) -> Result<Vec<XmlNode>, XmlError> {
    Ok(parse_xml_bytes(text.as_bytes())?.into_iter().map(to_utf8_node).collect())
}
