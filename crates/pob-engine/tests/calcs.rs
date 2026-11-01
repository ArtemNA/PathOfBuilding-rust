//! R8.6 gate: the Calcs tab (`pob_engine::calcs`) against LuaJIT (`tools/lua-corpus/export-calcs.ts`, fixtures `tests/fixtures/calcs/`).
//!
//! (1) data: `Modules/CalcSections.lua` as the Rust reader sees it = LuaJIT's `LoadModule("Modules/CalcSections")` (`_sections.json`, the probe's
//!     serialization `S`), and every section converts to the model (no key outside it).
//! (2) texts: for each scenario (the 52 of `crates/pob-calc/tests/calcs_mode.rs`, which gates the CALCS output / breakdown itself), every section
//!     of `calcsTab.sectionList` after `UpdateSize` (`enabled`, `height`) and every string `DrawContent` draws (`[line, text]`, in order), for
//!     the player and -- when `calcsEnv.minion` exists -- the minion. Exact.

use std::path::{Path, PathBuf};

use pob_engine::calcs::{self, DrawKind, LVal, SectionRender};
use pob_engine::display::DisplaySettings;
use pob_lua::format_g;
use serde_json::{Map, Value as Json, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calcs")
}

fn canon(v: &Json) -> String {
    match v {
        Json::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", Json::String((*k).clone()), canon(&m[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Json::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

/// The probe's `S` of a data value.
fn ser(v: &LVal) -> Json {
    match v {
        LVal::Str(s) => json!(s),
        LVal::Bool(b) => json!(b),
        LVal::Num(n) => json!({ "$n": format_g(*n, 17) }),
        LVal::Table(t) => {
            let mut o = Map::new();
            if !t.array.is_empty() {
                o.insert("[]".into(), Json::Array(t.array.iter().map(ser).collect()));
            }
            for (k, x) in &t.hash {
                o.insert(k.clone(), ser(x));
            }
            Json::Object(o)
        }
    }
}

#[test]
fn vendored_calc_sections_matches_upstream() {
    let upstream = Path::new(r"C:\pob-work\project_15\PathOfBuilding\src\Modules\CalcSections.lua");
    let Ok(text) = std::fs::read_to_string(upstream) else {
        eprintln!("upstream checkout absent, skipping");
        return;
    };
    assert_eq!(
        text,
        calcs::CALC_SECTIONS_LUA,
        "crates/pob-engine/upstream/CalcSections.lua drifted from upstream"
    );
}

#[test]
fn calc_sections_data_matches_luajit() {
    let fx: Json =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("_sections.json")).expect("_sections.json")).expect("json");
    let lua = fx["sections"].as_array().expect("sections");
    let rust = calcs::parse_calc_sections(calcs::CALC_SECTIONS_LUA).expect("parse");
    assert_eq!(rust.len(), lua.len(), "section count");
    let mut bad = 0;
    for (i, (r, l)) in rust.iter().zip(lua).enumerate() {
        if canon(&ser(r)) != canon(l) {
            bad += 1;
            println!(
                "section {i}: differs\n  rust   {}\n  luajit {}",
                &canon(&ser(r))[..400.min(canon(&ser(r)).len())],
                &canon(l)[..400.min(canon(l).len())]
            );
        }
    }
    // the model takes every key (SkillSelect + the data sections)
    let model = calcs::sections();
    let (subs, rows, cells): (usize, usize, usize) = model.iter().fold((0, 0, 0), |a, s| {
        let r: usize = s.sub_sections.iter().map(|x| x.rows.len()).sum();
        let c: usize = s.sub_sections.iter().flat_map(|x| &x.rows).map(|r| r.cells.len()).sum();
        (a.0 + s.sub_sections.len(), a.1 + r, a.2 + c)
    });
    println!(
        "data: {} sections (+ SkillSelect), {subs} subsections, {rows} rows, {cells} cells; differing {bad}",
        lua.len()
    );
    assert_eq!(model.len(), lua.len() + 1);
    assert_eq!(bad, 0);
}

fn xml_of(fx: &Json) -> String {
    let name = fx["build"].as_str().expect("build");
    let path = match fx["kind"].as_str().expect("kind") {
        "golden" => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
        "r81-synth" => Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../pob-calc/tests/fixtures/offence-synth/{name}.xml")),
        k => panic!("unknown kind {k}"),
    };
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn model_of(fx: &Json) -> pob_calc::pob_build::BuildModel {
    use pob_calc::pob_build::InputValue;
    let mut model = pob_calc::pob_build::load_build_xml(&xml_of(fx)).expect("xml");
    if let Some(ov) = fx["calcsInputOverride"].as_object() {
        for (k, v) in ov {
            let iv = match v {
                Json::String(s) => InputValue::Str(s.clone()),
                Json::Object(o) => InputValue::Num(o["$n"].as_str().expect("$n").parse().expect("number")),
                Json::Bool(b) => InputValue::Bool(*b),
                other => panic!("override {k}: {other}"),
            };
            model.calcs_input.insert(k.clone(), iv);
        }
    }
    model
}

#[derive(Default)]
struct Tally {
    sections: usize,
    sections_eq: usize,
    draws: usize,
    draws_eq: usize,
    bad: Vec<String>,
}

fn compare(slug: &str, who: &str, lua: &Json, rust: &[SectionRender], t: &mut Tally) {
    let lua = lua.as_array().expect("sections");
    if lua.len() != rust.len() {
        t.bad
            .push(format!("{slug} | {who} | {} sections, luajit {}", rust.len(), lua.len()));
        return;
    }
    for (l, r) in lua.iter().zip(rust) {
        t.sections += 1;
        let id = l["id"].as_str().expect("id");
        let mut ok = id == r.id
            && l["enabled"].as_bool() == Some(r.enabled)
            && l["height"].as_str() == Some(format_g(r.height, 17).as_str())
            && l.get("error").and_then(Json::as_str) == r.error.as_deref();
        if !ok {
            t.bad.push(format!(
                "{slug} | {who} | {id} | enabled/height/error: rust {} {} {:?}, luajit {} {} {:?}",
                r.enabled,
                r.height,
                r.error,
                l["enabled"],
                l["height"],
                l.get("error")
            ));
        }
        let ld = l["draws"].as_array().map(Vec::as_slice).unwrap_or_default();
        let n = ld.len().max(r.draws.len());
        for i in 0..n {
            let lv = ld.get(i).map(|d| (d[0].as_u64().expect("line"), d[1].as_str().expect("text")));
            let rv = r.draws.get(i).map(|(k, s)| (*k as u64, s.as_str()));
            if lv.is_some() {
                t.draws += 1;
            }
            match (lv, rv) {
                (Some(a), Some(b)) if a == b => t.draws_eq += 1,
                (a, b) => {
                    ok = false;
                    t.bad.push(format!("{slug} | {who} | {id} | draw {i}: rust {b:?}, luajit {a:?}"));
                }
            }
        }
        if ok {
            t.sections_eq += 1;
        }
    }
}

#[test]
fn calc_section_texts_match_luajit() {
    let _ = DrawKind::Cell;
    let mut list: Vec<String> = std::fs::read_dir(fixtures())
        .expect("fixtures")
        .filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".json").map(str::to_owned))
        .filter(|s| !s.starts_with('_'))
        .collect();
    list.sort();
    assert!(list.len() >= 52, "{} scenarios", list.len());
    let filter = std::env::var("CALCS_FILTER").ok();
    let st = DisplaySettings::default();
    let mut total = Tally::default();
    let (mut ran, mut minions) = (0, 0);
    println!("scenario -> sections / equal, draws / equal, divergences");
    for slug in &list {
        if filter.as_deref().is_some_and(|f| !slug.contains(f)) {
            continue;
        }
        ran += 1;
        let fx: Json =
            serde_json::from_str(&std::fs::read_to_string(fixtures().join(format!("{slug}.json"))).expect("fixture")).expect("json");
        let tab = calcs::build_output(model_of(&fx)).unwrap_or_else(|e| panic!("{slug}: {e}"));
        let run = tab.calcs.run.borrow();
        let (p, m) = calcs::render_run(&run, &st).unwrap_or_else(|e| panic!("{slug}: {e}"));
        let mut t = Tally::default();
        compare(slug, "player", &fx["sectionsPlayer"], &p, &mut t);
        match (&fx["sectionsMinion"], m) {
            (Json::Null, None) => {}
            (l, Some(m)) if !l.is_null() => {
                minions += 1;
                compare(slug, "minion", l, &m, &mut t);
            }
            (l, m) => t
                .bad
                .push(format!("{slug} | minion pass: luajit {}, rust {}", !l.is_null(), m.is_some())),
        }
        println!(
            "{slug:<50} {:>3} {:>3}  {:>4} {:>4}  {}",
            t.sections,
            t.sections_eq,
            t.draws,
            t.draws_eq,
            t.bad.len()
        );
        total.sections += t.sections;
        total.sections_eq += t.sections_eq;
        total.draws += t.draws;
        total.draws_eq += t.draws_eq;
        total.bad.extend(t.bad);
    }
    println!(
        "TOTAL {ran} scenarios ({minions} with the minion pass): sections {} equal {}, draws {} equal {}, divergences {}",
        total.sections,
        total.sections_eq,
        total.draws,
        total.draws_eq,
        total.bad.len()
    );
    let show = std::env::var("CALCS_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    for b in total.bad.iter().take(show) {
        println!("  {b}");
    }
    assert!(total.bad.is_empty(), "{} divergences", total.bad.len());
}

/// R8.6 perf: the section render (`UpdateSize` + `DrawContent` texts of every section, player + minion) of the CALCS run, median of 15 after
/// 3 warm-ups (LuaJIT twin: `node tools/lua-corpus/export-calcs.ts perf`, `render`).
/// Run: `cargo test -p pob-engine --release --test calcs -- --ignored --nocapture perf`.
#[test]
#[ignore = "perf"]
fn perf_render_sections() {
    let st = DisplaySettings::default();
    let mut list: Vec<String> = std::fs::read_dir(fixtures())
        .expect("fixtures")
        .filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".json").map(str::to_owned))
        .filter(|s| !s.starts_with('_'))
        .collect();
    list.sort();
    for slug in list {
        let fx: Json =
            serde_json::from_str(&std::fs::read_to_string(fixtures().join(format!("{slug}.json"))).expect("fixture")).expect("json");
        let tab = calcs::build_output(model_of(&fx)).expect("build");
        let run = tab.calcs.run.borrow();
        let mut xs = Vec::new();
        for i in 0..18 {
            let t0 = std::time::Instant::now();
            let r = calcs::render_run(&run, &st).expect("render");
            let el = t0.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(r);
            if i >= 3 {
                xs.push(el);
            }
        }
        xs.sort_by(f64::total_cmp);
        println!("PERF {slug:<50} render {:>8.3} ms", xs[xs.len() / 2]);
    }
}
