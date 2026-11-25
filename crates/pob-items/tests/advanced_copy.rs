//! Items pasted in the in-game advanced copy format (`{ <name> - <tags> }` headers, Item.lua:584-641, :1103-1154) = LuaJIT:
//! `new("Item"):Item(raw)` then BuildRaw (b1), BuildAndParseRaw then BuildRaw (b2), byte for byte. Fixture:
//! `tools/lua-corpus/export-advanced-copy.ts`.
//! R13: with several mods of one affix name and the same line (essence mods) LuaJIT takes the first in `pairs` (hash) order, so
//! its pick changes between processes; Rust takes the file order and lists the candidates on `unimplemented`. The gate checks
//! that LuaJIT's pick is one of those candidates and compares the rest of the text with Rust's pick in its place.

use pob_items::Item;

#[test]
fn advanced_copy_paste_equals_luajit() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/advanced-copy.luajit.json");
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let cases = j["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().expect("name");
        let mut item = Item::parse_with(c["raw"].as_str().expect("raw"), None, false);
        assert_eq!(
            item.implicit_mod_lines.len() as u64,
            c["implicits"].as_u64().expect("n"),
            "{name}: implicits"
        );
        assert_eq!(
            item.explicit_mod_lines.len() as u64,
            c["explicits"].as_u64().expect("n"),
            "{name}: explicits"
        );
        let picks = r13_picks(&item.unimplemented);
        assert_eq!(item.build_raw(), luajit(c["b1"].as_str().expect("b1"), &picks), "{name}: BuildRaw");
        item.build_and_parse_raw();
        assert_eq!(
            item.build_raw(),
            luajit(c["b2"].as_str().expect("b2"), &picks),
            "{name}: BuildRaw after BuildAndParseRaw"
        );
    }
    println!("advanced copy: {} pasted items = LuaJIT", cases.len());
}

/// The R13 notes of the parse: (the candidates, Rust's pick).
fn r13_picks(notes: &[String]) -> Vec<(Vec<String>, String)> {
    notes
        .iter()
        .filter_map(|n| n.split_once("R13: advanced copy affix candidates ").map(|(_, r)| r))
        .map(|r| {
            let (ids, took) = r.split_once("; took ").expect("note");
            (ids.split(',').map(str::to_owned).collect(), took.to_owned())
        })
        .collect()
}

/// LuaJIT's text with its R13 pick of each `Prefix:` / `Suffix:` line replaced by Rust's (the pick must be a candidate).
fn luajit(text: &str, picks: &[(Vec<String>, String)]) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let mut line = line.to_owned();
        if line.starts_with("Prefix: ") || line.starts_with("Suffix: ") {
            let id_at = line.rfind('}').map_or(8, |p| p + 1);
            let id = line[id_at..].to_owned();
            // a LuaJIT pick outside the candidates stays as it is, and the text comparison fails on it
            if let Some((_, took)) = picks.iter().find(|(ids, _)| ids.contains(&id)) {
                line = format!("{}{took}", &line[..id_at]);
            }
        }
        out.push(line);
    }
    out.join("\n")
}
