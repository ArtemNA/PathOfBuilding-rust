//! R8.2b gate on `tests/fixtures/buildraw.json` (tools/lua-corpus/export-buildraw.ts, LuaJIT docker):
//!  * `Item:BuildRaw` (Item.lua:1772) = LuaJIT byte for byte on every item of the 6 goldens + 20 R7-synth scenarios (built
//!    like ItemsTab:Load) and on the sampled uniques (built like Main.lua's uniqueDB, + the variant-1 switch);
//!  * `BuildAndParseRaw` (:2049): the texts after one and two re-parses = LuaJIT's (idempotency is LuaJIT's own fact, b2/b3);
//!  * the `<Item>` texts of the R8.1 `save.xml` fixtures (`ItemsTab:Save` :1349 after `BuildAndParseRaw`) = Rust BuildRaw.
//!
//! R13 (`pairs(self.baseLines)`, Item.lua:1951/1981; orchestrator's decision = the R13 rule of R8.1): items with 2+ written
//! base lines have no single LuaJIT text (the order of those lines is the process' string-hash order). The probe flags them
//! `r13` and stores the CANONICAL text (each run of base lines sorted bytewise, the rest verbatim). For them the gate is:
//! (a) the text outside the base-line runs = LuaJIT byte for byte; (b) the base-line runs = LuaJIT as a multiset (sorted);
//! (c) Rust writes each run in `base_lines` insertion (= text) order. Their count is pinned (20 + 20).

use std::path::{Path, PathBuf};

use pob_build::{ItemModel, XmlElement, XmlNode, load_build_xml, parse_xml_document, sanitise_text};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::{Item, ItemOptions};
use pob_parser::ParseMode;
use serde_json::Value as Json;

const MODE: ParseMode = ParseMode::Full;

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn fixture() -> Json {
    let p = manifest().join("tests/fixtures/buildraw.json");
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("buildraw.json")
}

fn scenario_xml(slug: &str, kind: &str) -> PathBuf {
    match kind {
        "golden" => GOLDEN_BUILDS
            .iter()
            .find(|g| g.name.replace(' ', "-") == slug)
            .unwrap_or_else(|| panic!("golden {slug}"))
            .xml_path(),
        _ => manifest().join(format!("../pob-calc/tests/fixtures/offence-synth/{slug}.xml")),
    }
}

fn options_of(im: &ItemModel) -> ItemOptions {
    ItemOptions {
        parse_mode: MODE,
        id: Some(im.id),
        variant: im.variant,
        variant_alt: [im.variant_alt, im.variant_alt2, im.variant_alt3, im.variant_alt4, im.variant_alt5],
        mod_ranges: im.mod_ranges.iter().map(|r| (r.id, r.range)).collect(),
    }
}

/// One BuildRaw text with the base-line runs of the item's state at that moment located (export-buildraw.ts `canon`): a
/// run is a maximal sequence of lines, from line 4 on, whose text after its leading `{..}` tags is a `base_lines` key.
struct Built {
    raw: String,
    /// The canonical form: every run sorted bytewise, the rest verbatim.
    canon: String,
    /// (c): every run is in `base_lines` insertion order.
    text_order: bool,
}

fn strip_tags(mut l: &str) -> &str {
    while l.starts_with('{') {
        match l.find('}') {
            Some(e) => l = &l[e + 1..],
            None => break,
        }
    }
    l
}

/// (runs, outside lines) of `text` for the base-line keys `keys`.
fn split_runs<'a>(text: &'a str, keys: &[&str]) -> (Vec<Vec<&'a str>>, Vec<Option<&'a str>>) {
    let lines: Vec<&str> = text.split('\n').collect();
    let is_base = |i: usize| i >= 3 && keys.contains(&strip_tags(lines[i]));
    let (mut runs, mut outside) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < lines.len() {
        if is_base(i) {
            let mut run = Vec::new();
            while i < lines.len() && is_base(i) {
                run.push(lines[i]);
                i += 1;
            }
            runs.push(run);
            outside.push(None); // the run's place
        } else {
            outside.push(Some(lines[i]));
            i += 1;
        }
    }
    (runs, outside)
}

fn canon(text: &str, keys: &[&str]) -> String {
    let (mut runs, outside) = split_runs(text, keys);
    let mut runs = runs.iter_mut();
    let mut out: Vec<&str> = Vec::new();
    for o in outside {
        match o {
            Some(l) => out.push(l),
            None => {
                let run = runs.next().expect("run");
                run.sort_unstable();
                out.extend(run.iter());
            }
        }
    }
    out.join("\n")
}

fn built(item: &Item) -> Built {
    let raw = item.build_raw();
    let keys: Vec<&str> = item.base_lines.keys().map(String::as_str).collect();
    let (runs, _) = split_runs(&raw, &keys);
    let text_order = runs.iter().all(|run| {
        let pos: Vec<usize> = run
            .iter()
            .map(|l| keys.iter().position(|k| *k == strip_tags(l)).expect("key"))
            .collect();
        pos.windows(2).all(|w| w[0] < w[1])
    });
    Built {
        canon: canon(&raw, &keys),
        raw,
        text_order,
    }
}

/// The probe's sequence: b1 = BuildRaw(); BuildAndParseRaw(); b2; BuildAndParseRaw(); b3.
fn run_seq(item: &mut Item) -> [Built; 3] {
    let b1 = built(item);
    item.build_and_parse_raw();
    let b2 = built(item);
    item.build_and_parse_raw();
    let b3 = built(item);
    [b1, b2, b3]
}

/// The fixture's `{b1, b2?, b3?}` expanded (an absent b2/b3 = the previous text).
fn expected_seq(j: &Json) -> [String; 3] {
    assert!(j.get("err").is_none(), "LuaJIT BuildRaw raised: {j}");
    let b1 = j["b1"].as_str().expect("b1").to_owned();
    let b2 = j.get("b2").and_then(Json::as_str).map_or_else(|| b1.clone(), str::to_owned);
    let b3 = j.get("b3").and_then(Json::as_str).map_or_else(|| b2.clone(), str::to_owned);
    [b1, b2, b3]
}

#[derive(Default, Debug)]
struct Tally {
    items: usize,
    equal: usize,
    r13: usize,
    /// (a)+(b): canonical text = LuaJIT's canonical text.
    r13_canon_equal: usize,
    /// (c): base-line runs in text order.
    r13_text_order: usize,
    bad: Vec<String>,
}

fn first_diff(what: &str, i: usize, g: &str, e: &str) -> String {
    let gl: Vec<&str> = g.split('\n').collect();
    let el: Vec<&str> = e.split('\n').collect();
    let at = (0..gl.len().max(el.len())).find(|&k| gl.get(k) != el.get(k)).unwrap_or(0);
    format!(
        "{what} b{}: line {}: rust {:?} / luajit {:?}",
        i + 1,
        at + 1,
        gl.get(at),
        el.get(at)
    )
}

impl Tally {
    fn check(&mut self, what: &str, got: &[Built; 3], exp: &[String; 3], r13: bool) {
        self.items += 1;
        if r13 {
            self.r13 += 1;
            match (0..3).find(|&i| got[i].canon != exp[i]) {
                None => self.r13_canon_equal += 1,
                Some(i) => self
                    .bad
                    .push(first_diff(&format!("{what} (R13 canonical)"), i, &got[i].canon, &exp[i])),
            }
            if got.iter().all(|b| b.text_order) {
                self.r13_text_order += 1;
            } else {
                self.bad.push(format!("{what}: R13 base lines not in text order"));
            }
            return;
        }
        match (0..3).find(|&i| got[i].raw != exp[i]) {
            None => self.equal += 1,
            Some(i) => self.bad.push(first_diff(what, i, &got[i].raw, &exp[i])),
        }
    }
}

fn build_items(xml_path: &Path) -> Vec<Item> {
    let xml = std::fs::read_to_string(xml_path).unwrap_or_else(|e| panic!("{}: {e}", xml_path.display()));
    let model = load_build_xml(&xml).unwrap_or_else(|e| panic!("{}: {e}", xml_path.display()));
    model
        .items
        .iter()
        .map(|im| Item::parse(&im.raw, &options_of(im)))
        .filter(|i| i.base.is_some())
        .collect()
}

fn uniques_json() -> Json {
    pob_data::read_json("uniques.json").unwrap_or_else(|e| panic!("{e}"))
}

/// Main.lua:166 `new("Item"):Item(raw, "UNIQUE", true)` (Item:Item sanitises).
fn unique_item(raw: &str) -> Item {
    let mut item = Item::default();
    item.parse_mode = MODE;
    item.parse_raw(&sanitise_text(raw), Some("UNIQUE"), true);
    item
}

#[test]
fn build_raw_equals_luajit_on_build_items() {
    let fx = fixture();
    let mut t = Tally::default();
    let (mut crafted, mut affix_r13) = (0, 0);
    for b in fx["builds"].as_array().expect("builds") {
        let slug = b["slug"].as_str().expect("slug");
        let mut items = build_items(&scenario_xml(slug, b["kind"].as_str().expect("kind")));
        let exp = b["items"].as_array().expect("items");
        assert_eq!(items.len(), exp.len(), "{slug}: item count");
        for (item, e) in items.iter_mut().zip(exp) {
            assert_eq!(item.id, e["id"].as_f64(), "{slug}: item order");
            let got = run_seq(item);
            crafted += usize::from(item.crafted);
            affix_r13 += usize::from(item.unimplemented.iter().any(|u| u.contains(":1506 R13")));
            t.check(&format!("{slug} #{}", e["id"]), &got, &expected_seq(e), e.get("r13").is_some());
        }
    }
    println!("build items: {t:?}; crafted (Prefix/Suffix slots written) {crafted}, ambiguous affix name (R13) {affix_r13}");
    assert!(t.bad.is_empty(), "{} of {} differ:\n{}", t.bad.len(), t.items, t.bad.join("\n"));
    assert_eq!((t.items, t.equal, t.r13), (534, 534, 0), "corpus size / R13 items");
}

#[test]
fn build_raw_equals_luajit_on_uniques() {
    let fx = fixture();
    let uj = uniques_json();
    let (mut seq, mut var1) = (Tally::default(), Tally::default());
    for u in fx["uniques"].as_array().expect("uniques") {
        let ty = u["type"].as_str().expect("type");
        let index = u["index"].as_u64().expect("index") as usize;
        let raw = uj["byType"][ty][index]
            .as_str()
            .unwrap_or_else(|| panic!("uniques.json {ty}[{index}]"));
        let what = format!("{ty}[{index}]");
        let mut item = unique_item(raw);
        assert_eq!(item.base.is_none(), u.get("noBase").is_some(), "{what}: base resolved");
        let Some(e) = u.get("seq") else { continue };
        let got = run_seq(&mut item);
        seq.check(&what, &got, &expected_seq(e), e.get("r13").is_some());
        let item2 = unique_item(raw);
        let has_variants = item2.variant_list.as_ref().is_some_and(|v| v.len() > 1);
        assert_eq!(has_variants, u.get("variant1").is_some(), "{what}: variant flow");
        if let Some(e) = u.get("variant1") {
            let mut item2 = item2;
            item2.variant = Some(1.0);
            item2.build_and_parse_raw();
            let got = run_seq(&mut item2);
            var1.check(&format!("{what} variant 1"), &got, &expected_seq(e), e.get("r13").is_some());
        }
    }
    println!("uniques: {seq:?}\nuniques, variant 1: {var1:?}");
    let bad: Vec<String> = seq.bad.iter().chain(&var1.bad).cloned().collect();
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
    assert_eq!(
        (seq.items, seq.r13, var1.items, var1.r13),
        // 3dbca4463: 271 uniques with variants (LuaJIT `uniquesWithVariant1`, export-buildraw.ts)
        (450, 20, 271, 20),
        "corpus size / R13 items"
    );
}

fn find_items<'a>(e: &'a XmlElement, out: &mut Vec<&'a XmlElement>) {
    for c in e.child_elements() {
        if c.name == "Item" {
            out.push(c);
        } else {
            find_items(c, out);
        }
    }
}

/// R8.1 `save.xml`: `<Item>` text = `item.raw` after `BuildAndParseRaw` = BuildRaw of the loaded item.
#[test]
fn save_xml_item_texts_equal_rust_build_raw() {
    let fx = fixture();
    let (mut scenarios, mut total, mut bad) = (0, 0, Vec::new());
    for b in fx["builds"].as_array().expect("builds") {
        let slug = b["slug"].as_str().expect("slug");
        let p = manifest().join(format!("../pob-engine/tests/fixtures/r8/{slug}.save.xml"));
        let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let doc = parse_xml_document(&text).unwrap_or_else(|e| panic!("{slug}: {e}"));
        let mut saved = Vec::new();
        for n in &doc {
            if let XmlNode::Element(e) = n {
                find_items(e, &mut saved);
            }
        }
        let items = build_items(&scenario_xml(slug, b["kind"].as_str().expect("kind")));
        assert_eq!(saved.len(), items.len(), "{slug}: <Item> count");
        for s in saved {
            let id: f64 = s.attr("id").and_then(|x| x.parse().ok()).expect("id");
            let item = items
                .iter()
                .find(|i| i.id == Some(id))
                .unwrap_or_else(|| panic!("{slug}: item {id}"));
            let first_text = s
                .children
                .iter()
                .find_map(|c| if let XmlNode::Text(t) = c { Some(t.as_str()) } else { None })
                .expect("item text");
            // The composer frames the text with indentation (`\n\t\t\t` .. `\n\t\t`); BuildRaw never starts/ends with these.
            let saved_text = first_text.trim_matches(|c| c == '\n' || c == '\t');
            let got = item.build_raw();
            assert_eq!(got.trim_matches(|c| c == '\n' || c == '\t'), got, "{slug} #{id}: framing");
            total += 1;
            if saved_text != got {
                bad.push(format!("{slug} #{id}"));
            }
        }
        scenarios += 1;
    }
    println!("save.xml: {scenarios} scenarios, {total} <Item> texts, {} differ", bad.len());
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!((scenarios, total), (26, 534));
}

/// Perf: `cargo test -p pob-items --release --test buildraw -- --ignored --nocapture perf`.
#[test]
#[ignore]
fn perf_build_raw() {
    let fx = fixture();
    let uj = uniques_json();
    let mut items: Vec<Item> = Vec::new();
    for b in fx["builds"].as_array().expect("builds") {
        items.extend(build_items(&scenario_xml(
            b["slug"].as_str().expect("slug"),
            b["kind"].as_str().expect("kind"),
        )));
    }
    for u in fx["uniques"].as_array().expect("uniques") {
        let raw = uj["byType"][u["type"].as_str().expect("type")][u["index"].as_u64().expect("index") as usize]
            .as_str()
            .expect("raw");
        let it = unique_item(raw);
        if it.base.is_some() {
            items.push(it);
        }
    }
    let n = items.len() as f64;
    let reps = 10;
    let (mut br, mut bpr) = (Vec::new(), Vec::new());
    for r in 0..=reps {
        let t0 = std::time::Instant::now();
        let mut len = 0;
        for it in &items {
            len += std::hint::black_box(it.build_raw()).len();
        }
        let t1 = std::time::Instant::now();
        for it in &mut items {
            it.build_and_parse_raw();
        }
        let t2 = std::time::Instant::now();
        std::hint::black_box(len);
        if r > 0 {
            br.push((t1 - t0).as_nanos() as f64 / n);
            bpr.push((t2 - t1).as_nanos() as f64 / n);
        }
    }
    let med = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    println!(
        "items {n}: BuildRaw median {:.0} ns/item (min {:.0}); BuildAndParseRaw median {:.0} ns/item (min {:.0})",
        med(&mut br),
        br[0],
        med(&mut bpr),
        bpr[0]
    );
}
