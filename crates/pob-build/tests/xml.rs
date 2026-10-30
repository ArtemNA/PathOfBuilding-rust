//! pob-ts `test/build/xml.test.ts`, case for case. The three D22 tests (9 expectations) were checked by pob-ts on the
//! oracle's own `xml.ParseXML` (9/9) -- copied here verbatim.

use pob_build::{XmlNode, parse_xml};

fn text(x: &str) -> String {
    parse_xml(x).unwrap().text()
}

#[test]
fn simple_element_with_attributes() {
    let root = parse_xml(r#"<Foo a="1" b="2"/>"#).unwrap();
    assert_eq!(root.name, "Foo");
    assert_eq!(
        root.attrib.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(),
        [("a", "1"), ("b", "2")]
    );
    assert!(root.children.is_empty());
}

#[test]
fn self_closing_open_close_and_quotes() {
    assert_eq!(parse_xml("<Foo a='x'/>").unwrap().attr("a"), Some("x"));
    assert_eq!(parse_xml(r#"<Foo a="x"></Foo>"#).unwrap().attr("a"), Some("x"));
}

#[test]
fn child_elements_skip_text() {
    let root = parse_xml("<Root>text<A/>more<B/></Root>").unwrap();
    assert_eq!(root.child_elements().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
}

#[test]
fn text_is_verbatim() {
    assert_eq!(
        text("<Item>\n\tRarity: UNIQUE\n\tThe Saviour\n\t</Item>"),
        "\n\tRarity: UNIQUE\n\tThe Saviour\n\t"
    );
}

#[test]
fn text_runs_between_siblings() {
    let root = parse_xml("<A>x<B/>y<C/>z</A>").unwrap();
    assert_eq!(root.children.len(), 5);
    assert!(matches!(&root.children[0], XmlNode::Text(t) if t == "x"));
    assert_eq!(root.text(), "xyz");
}

#[test]
fn standard_entities() {
    let root = parse_xml(r#"<A v="&lt;&gt;&amp;&quot;&apos;">&lt;&gt;&amp;&quot;&apos;</A>"#).unwrap();
    assert_eq!(root.attr("v"), Some("<>&\"'"));
    assert_eq!(root.text(), "<>&\"'");
}

// --- D22 (9 expectations, oracle-checked) ---------------------------------------------------------------------------

#[test]
fn d22_numeric_hex_and_unknown_are_erased() {
    let root = parse_xml(r#"<A v="x&#10;y&foo;z">Life&#10;Mana&#xA;ES&#65;&nbsp;!</A>"#).unwrap();
    assert_eq!(root.attr("v"), Some("xyz"));
    assert_eq!(root.text(), "LifeManaES!");
}

#[test]
fn d22_lazy_to_first_semicolon_lone_ampersand_literal() {
    assert_eq!(text("<A>a & b; c</A>"), "a  c");
    assert_eq!(text("<A>a &amp\n;b</A>"), "a b");
    assert_eq!(text("<A>Fish & Chips</A>"), "Fish & Chips");
    assert_eq!(text("<A>&&amp;;</A>"), ";");
    assert_eq!(parse_xml(r#"<A v="R&D"/>"#).unwrap().attr("v"), Some("R&D"));
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
    assert_eq!(parse_xml("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<A/>").unwrap().name, "A");
    assert_eq!(parse_xml("\u{feff}<?xml version=\"1.0\"?><A/>").unwrap().name, "A");
}

#[test]
fn crlf_and_cr_normalised() {
    assert_eq!(text("<A>line1\r\nline2\rline3</A>"), "line1\nline2\nline3");
}

#[test]
fn whitespace_between_attributes() {
    let root = parse_xml("<A\n  a=\"1\"\n  b=\"2\"\n/>").unwrap();
    assert_eq!((root.attr("a"), root.attr("b")), (Some("1"), Some("2")));
}

#[test]
fn errors() {
    assert!(parse_xml("<A><B></C></A>").is_err());
    assert!(parse_xml("<A><B>").is_err());
    assert!(parse_xml("<A><!-- never closed</A>").is_err());
    let e = parse_xml("<A>\n<B>\nx</C></A>").unwrap_err();
    assert_eq!((e.line, e.column), (3, 6));
}

#[test]
fn apos_in_item_text_and_utf8_kept() {
    assert!(text("<Item id=\"1\">\nCospri&apos;s Malice\n</Item>").contains("Cospri's Malice"));
    assert_eq!(text("<A>Mj\u{f6}lner</A>"), "Mj\u{f6}lner");
}

#[test]
fn independent_parses() {
    let a = parse_xml(r#"<A x="1"/>"#).unwrap();
    let b = parse_xml(r#"<B y="2"/>"#).unwrap();
    assert_eq!(
        (a.name.as_str(), a.attr("x"), b.name.as_str(), b.attr("y")),
        ("A", Some("1"), "B", Some("2"))
    );
}
