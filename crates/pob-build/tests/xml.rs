//! Hand-written cases of the build-XML parser. R3.2 copied them from pob-ts `test/build/xml.test.ts`; R3.8b ported
//! `runtime/lua/xml.lua` verbatim (the full truth is the LuaJIT corpus in `xml_lua.rs`), so the cases where pob-ts and Lua
//! differ now state the Lua result (marked "Lua:"). The three D22 tests (9 expectations, oracle-checked by pob-ts on the
//! oracle's own `xml.ParseXML`) are unchanged.

use pob_build::{XmlElement, XmlNode, parse_xml_document};

/// `ParseXML(x)[1]`, which must be an element.
fn root(x: &str) -> XmlElement {
    match parse_xml_document(x).unwrap().into_iter().next() {
        Some(XmlNode::Element(e)) => e,
        other => panic!("{x:?}: first node {other:?}"),
    }
}

fn text(x: &str) -> String {
    root(x).text()
}

#[test]
fn simple_element_with_attributes() {
    let root = root(r#"<Foo a="1" b="2"/>"#);
    assert_eq!(root.name, "Foo");
    assert_eq!(
        root.attrib.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(),
        [("a", "1"), ("b", "2")]
    );
    assert!(root.children.is_empty() && root.empty);
}

#[test]
fn self_closing_open_close_and_quotes() {
    assert_eq!(root("<Foo a='x'/>").attr("a"), Some("x"));
    assert_eq!(root(r#"<Foo a="x"></Foo>"#).attr("a"), Some("x"));
}

#[test]
fn child_elements_skip_text() {
    let root = root("<Root>text<A/>more<B/></Root>");
    assert_eq!(root.child_elements().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
}

#[test]
fn text_is_trimmed() {
    // Lua: `^%s*([^<]-)%s*<` trims each text run (pob-ts kept it verbatim).
    assert_eq!(
        text("<Item>\n\tRarity: UNIQUE\n\tThe Saviour\n\t</Item>"),
        "Rarity: UNIQUE\n\tThe Saviour"
    );
}

#[test]
fn text_runs_between_siblings() {
    let root = root("<A>x<B/>y<C/>z</A>");
    assert_eq!(root.children.len(), 5);
    assert!(matches!(&root.children[0], XmlNode::Text(t) if t == "x"));
    assert_eq!(root.text(), "xyz");
}

#[test]
fn standard_entities() {
    let root = root(r#"<A v="&lt;&gt;&amp;&quot;&apos;">&lt;&gt;&amp;&quot;&apos;</A>"#);
    assert_eq!(root.attr("v"), Some("<>&\"'"));
    assert_eq!(root.text(), "<>&\"'");
}

// --- D22 (9 expectations, oracle-checked) ---------------------------------------------------------------------------

#[test]
fn d22_numeric_hex_and_unknown_are_erased() {
    let root = root(r#"<A v="x&#10;y&foo;z">Life&#10;Mana&#xA;ES&#65;&nbsp;!</A>"#);
    assert_eq!(root.attr("v"), Some("xyz"));
    assert_eq!(root.text(), "LifeManaES!");
}

#[test]
fn d22_lazy_to_first_semicolon_lone_ampersand_literal() {
    assert_eq!(text("<A>a & b; c</A>"), "a  c");
    assert_eq!(text("<A>a &amp\n;b</A>"), "a b");
    assert_eq!(text("<A>Fish & Chips</A>"), "Fish & Chips");
    assert_eq!(text("<A>&&amp;;</A>"), ";");
    assert_eq!(root(r#"<A v="R&D"/>"#).attr("v"), Some("R&D"));
}

#[test]
fn d22_run_spans_comments_not_cdata() {
    assert_eq!(text("<A>x&<!-- c -->y;z</A>"), "xz");
    assert_eq!(text("<A>x&<![CDATA[;]]>y;</A>"), "x&;y;");
}

// ----------------------------------------------------------------------------------------------------------------------

#[test]
fn cdata_is_literal() {
    assert_eq!(text("<A><![CDATA[a & <b> &amp;]]></A>"), "a & <b> &amp;");
}

#[test]
fn comments_anywhere() {
    assert_eq!(text("<!-- lead --><A><!-- mid -->x</A><!-- trail -->"), "x");
}

#[test]
fn xml_declaration_and_bom() {
    assert_eq!(root("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<A/>").name, "A");
    // Lua: a BOM is a text node before the root (Build.lua then reports "'PathOfBuilding' root element missing").
    let top = parse_xml_document("\u{feff}<?xml version=\"1.0\"?><A/>").unwrap();
    assert!(matches!(&top[0], XmlNode::Text(t) if t == "\u{feff}"));
    assert!(matches!(&top[1], XmlNode::Element(e) if e.name == "A"));
}

#[test]
fn crlf_is_not_normalised() {
    // Lua: no newline handling; `%s` trims the CR/LF at the edges of a run only.
    assert_eq!(text("<A>line1\r\nline2\rline3\r\n</A>"), "line1\r\nline2\rline3");
}

#[test]
fn whitespace_between_attributes() {
    let root = root("<A\n  a=\"1\"\n  b=\"2\"\n/>");
    assert_eq!((root.attr("a"), root.attr("b")), (Some("1"), Some("2")));
}

#[test]
fn errors() {
    let msg = |x: &str| parse_xml_document(x).unwrap_err().message;
    assert_eq!(msg("<A><B></C></A>"), "trying to close <B> with 'C'");
    assert_eq!(msg("<A><B>"), "unclosed element 'B'");
    assert_eq!(msg("<A><!-- never closed</A>"), "unclosed element 'A'");
    assert_eq!(msg("</A>"), "nothing to close with 'A'");
}

#[test]
fn apos_in_item_text_and_utf8_kept() {
    assert!(text("<Item id=\"1\">\nCospri&apos;s Malice\n</Item>").contains("Cospri's Malice"));
    assert_eq!(text("<A>Mj\u{f6}lner</A>"), "Mj\u{f6}lner");
}

#[test]
fn independent_parses() {
    let a = root(r#"<A x="1"/>"#);
    let b = root(r#"<B y="2"/>"#);
    assert_eq!(
        (a.name.as_str(), a.attr("x"), b.name.as_str(), b.attr("y")),
        ("A", Some("1"), "B", Some("2"))
    );
}
