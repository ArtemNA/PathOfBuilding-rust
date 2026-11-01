//! R8.3 gate: the sidebar stats = LuaJIT, scenario by scenario, line by line.
//!
//! Fixtures `tests/fixtures/r8/<slug>.display.json` (producer `tools/lua-corpus/export-r8.ts`, R8.1): after `loadBuildFromXML` +
//! `wipeGlobalCache` + `calcsTab:BuildOutput()` + `build:RefreshStatList()` in the LuaJIT docker --
//! `sections` (the `engine_displayStatsJSON` structure, boot.lua:546-788), `rawPlayer` / `rawMinion` (every `displayStats` entry with its
//! functions evaluated), `statBox` (`controls.statBox.list`), `warnings` (sorted: R13), `itemWarnings`, `infoMessage*`, `disableReason`.
//! Rust: `calculate_with(<the same XML bytes>)` (raw text, as the docker read it) -> `build_display`.
//!
//! Every field is compared exactly (strings byte for byte, numbers as `%.17g` strings), with one stated exception: the second
//! `underline` flag of a stat row needs the contents of `env.<actor>.breakdown` (Build.lua:1881-1902), which `build_display` does not
//! read ([`BreakdownKey::Unported`]). R8.4: the test decides it itself from Rust's breakdown (`GetStatBreakdownKey`, [`rust_underline`])
//! when the key's breakdown entry is one perform / defence / estimations build (class `ported` in `pob-calc/tests/fixtures/breakdown`,
//! or no module writes it) -- compared with LuaJIT like every field; the keys offence / triggers write (R8.5) stay UNVERIFIED (counted),
//! every other field of that entry is still compared.

use std::path::{Path, PathBuf};
use std::time::Instant;

use pob_calc::{CalculateOptions, CalculateResult, OutTable, calculate_with};
use pob_engine::display::{
    BreakdownKey, DisplayEnv, DisplaySection, DisplaySettings, MinionBorrow, StatBoxEntry, build_display, display_sections, match_flags,
    refresh_stat_list,
};
use pob_engine::lua::{LuaV, TableRef};
use pob_engine::stats::{DISPLAY_STATS, DisplayStat, MINION_DISPLAY_STATS, Suffix};
use pob_engine::view::ActorView;
use pob_lua::format_g;
use serde_json::{Map, Value as Json, json};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r8")
}

fn synth_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-calc/tests/fixtures/offence-synth")
}

fn scenarios() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .expect("fixtures dir")
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".display.json")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v
}

fn fixture(slug: &str) -> Json {
    serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{slug}.display.json"))).expect("fixture")).expect("json")
}

/// The XML the probe loaded: a golden's file, or an R7-synth scenario (raw bytes, as `readFileSync` handed them to the docker).
fn xml_of(fx: &Json) -> String {
    let name = fx["build"].as_str().expect("build");
    let path = match fx["kind"].as_str() {
        Some("golden") => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
        _ => synth_dir().join(format!("{name}.xml")),
    };
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn calc(fx: &Json) -> CalculateResult {
    calculate_with(&xml_of(fx), CalculateOptions::default()).unwrap_or_else(|e| panic!("{}: {e}", fx["build"]))
}

// --- the probe's serialisation (export-r8.ts HELPERS `N` / `D`) -------------------------------------------------------------------

fn num(v: f64) -> Json {
    Json::String(format_g(v, 17))
}

/// `D(string)`: a string that looks like a number gets the `$str:` marker.
fn dstr(s: &str) -> Json {
    let numeric = matches!(s, "nan" | "inf" | "-inf")
        || pob_lua::find(s.as_bytes(), b"^%-?[%d%.]+[eE]?[%+%-]?%d*$", None, false)
            .ok()
            .flatten()
            .is_some();
    Json::String(if numeric { format!("$str:{s}") } else { s.to_owned() })
}

fn dlist(l: &[&str]) -> Json {
    Json::Array(l.iter().map(|s| dstr(s)).collect())
}

/// `D(v)` of a function result.
fn dval(v: &LuaV<'_>) -> Json {
    match v {
        LuaV::Nil => json!("<nil>"),
        LuaV::Bool(b) => json!(b),
        LuaV::Num(n) => num(*n),
        LuaV::Str(s) => dstr(s),
        LuaV::Table(_) => json!("<table>"),
    }
}

fn tostring(v: &LuaV<'_>) -> String {
    match v {
        LuaV::Nil => "nil".into(),
        LuaV::Bool(b) => b.to_string(),
        LuaV::Num(n) => pob_lua::tostring_num(*n),
        LuaV::Str(s) => s.to_string(),
        LuaV::Table(_) => "table".into(),
    }
}

/// export-r8.ts PROBE `rawList`: one entry.
fn raw_entry(i: usize, sd: &DisplayStat, o: &ActorView<'_>, flags: &indexmap::IndexMap<String, bool>) -> Json {
    let mut e = Map::new();
    e.insert("index".into(), num(i as f64));
    let mut put = |k: &str, v: Option<Json>| {
        if let Some(v) = v {
            e.insert(k.into(), v);
        }
    };
    let f = || Some(json!("<function>"));
    let t = |b: bool| b.then(|| json!(true));
    put("stat", sd.stat.map(dstr));
    put("childStat", sd.child_stat.map(dstr));
    put("label", sd.label.map(dstr));
    put("fmt", sd.fmt.map(dstr));
    put("breakdown", sd.breakdown.map(dstr));
    put("modNames", sd.mod_names.map(dlist));
    put("ignoredSections", sd.ignored_sections.map(dlist));
    put("color", sd.color.map(dstr));
    put("flag", sd.flag.map(dstr));
    put("notFlag", sd.not_flag.map(dstr));
    put("condFunc", sd.cond_func.and_then(|_| f()));
    put("warnFunc", sd.warn_func.and_then(|_| f()));
    put("warnColor", t(sd.warn_color));
    put(
        "suffix",
        sd.suffix.map(|s| match s {
            Suffix::Str(x) => dstr(x),
            Suffix::Fn(_) => json!("<function>"),
        }),
    );
    put("suffixCondFunc", sd.suffix_cond_func.and_then(|_| f()));
    put("overCapStat", sd.over_cap_stat.map(dstr));
    put("overCapStatCondFunc", sd.over_cap_stat_cond_func.and_then(|_| f()));
    put("compactValue", t(sd.compact_value));
    put("compPercent", t(sd.comp_percent));
    put("lowerIsBetter", t(sd.lower_is_better));
    put("pc", t(sd.pc));
    put("mod", t(sd.mod_));
    put("hideStat", t(sd.hide_stat));
    put("chaosInoc", t(sd.chaos_inoc));
    put("pool", sd.pool.map(dstr));
    put("labelStat", sd.label_stat.map(dstr));
    put("val", sd.val.map(dstr));
    put("separator", t(sd.stat.is_none() && sd.label.is_none()));
    put("flagMatch", Some(json!(match_flags(sd.flag, sd.not_flag, flags))));
    if let Some(stat) = sd.stat {
        let mut v = o.get(stat);
        if v.truthy()
            && let Some(c) = sd.child_stat
        {
            v = v.index(c).expect("childStat");
        }
        put("valueType", Some(json!(v.type_name())));
        if let LuaV::Num(x) = v {
            put("value", Some(num(x)));
        }
        if let LuaV::Table(tr) = v {
            let len = match tr {
                TableRef::Out(OutTable::SkillDps(l)) => l.len(),
                TableRef::Out(OutTable::Names(l)) => l.len(),
                _ => 0,
            };
            put("valueLen", Some(num(len as f64)));
        }
        let present = !matches!(v, LuaV::Nil);
        if present {
            if let Some(c) = sd.cond_func {
                put(
                    "condResult",
                    Some(match c(&v, o) {
                        Ok(r) if r.truthy() => match r {
                            LuaV::Bool(b) => json!(b),
                            other => dval(&other),
                        },
                        Ok(r) => json!(format!("<error {}>", tostring(&r))),
                        Err(m) => json!(format!("<error {m}>")),
                    }),
                );
            }
            if let Some(w) = sd.warn_func {
                put(
                    "warnResult",
                    Some(match w(&v, o) {
                        Ok(LuaV::Bool(false)) => json!("<error false>"),
                        Ok(r) => dval(&r),
                        Err(m) => json!(format!("<error {m}>")),
                    }),
                );
            }
            if let Some(c) = sd.suffix_cond_func {
                put(
                    "suffixCondResult",
                    Some(match c(&v, o) {
                        Ok(r) if r.truthy() => match r {
                            LuaV::Bool(b) => json!(b),
                            other => dval(&other),
                        },
                        _ => json!("<error>"),
                    }),
                );
            }
            if let Some(Suffix::Fn(sf)) = sd.suffix {
                put(
                    "suffixResult",
                    Some(match sf(&v, o) {
                        Ok(LuaV::Bool(false)) | Err(_) => json!("<error>"),
                        Ok(r) => dval(&r),
                    }),
                );
            }
        }
        if let Some(oc) = sd.over_cap_stat {
            let x = o.get(oc);
            put(
                "overCapValue",
                match x {
                    LuaV::Nil | LuaV::Bool(false) => None,
                    other => Some(dval(&other)),
                },
            );
        }
        put("hasWarningKey", Some(json!(o.has(&format!("{stat}Warning")))));
    }
    Json::Object(e)
}

fn stat_box_json(e: &StatBoxEntry) -> Json {
    let mut m = Map::new();
    m.insert("height".into(), num(e.height));
    if let Some(a) = e.align {
        m.insert("align".into(), json!(a));
    }
    if let Some(x) = e.x {
        m.insert("x".into(), num(x));
    }
    if let Some(t) = &e.text {
        m.insert("text".into(), json!(t));
    }
    if let Some(v) = &e.value {
        m.insert("value".into(), json!(v));
    }
    if let Some(u) = e.underline {
        m.insert(
            "underline".into(),
            Json::Array(u.iter().map(|b| b.map_or(Json::Null, Json::Bool)).collect()),
        );
    }
    if let Some(a) = e.actor_name {
        m.insert("actorName".into(), json!(a));
    }
    if let Some(mn) = e.mod_names {
        m.insert("modNames".into(), dlist(mn));
    }
    if let Some(is) = e.ignored_sections {
        let mut k: Vec<&str> = is.to_vec();
        k.sort_unstable();
        k.dedup();
        m.insert("ignoredSections".into(), json!(k));
    }
    if let Some(c) = e.breakdown_colour {
        m.insert("breakdownColour".into(), Json::Array(c.iter().map(|x| num(*x)).collect()));
    }
    Json::Object(m)
}

fn sections_json(s: &[DisplaySection]) -> Json {
    Json::Array(
        s.iter()
            .map(|sec| {
                let rows: Vec<Json> = sec
                    .rows
                    .iter()
                    .map(|r| {
                        let mut m = Map::new();
                        m.insert("label".into(), json!(r.label));
                        m.insert("value".into(), json!(r.value));
                        m.insert("color".into(), json!(r.color));
                        m.insert("valueColor".into(), json!(r.value_color));
                        if let Some(k) = r.stat_key {
                            m.insert("statKey".into(), json!(k));
                        }
                        Json::Object(m)
                    })
                    .collect();
                json!({ "actor": sec.actor, "title": sec.title, "slot": sec.slot, "rows": rows })
            })
            .collect(),
    )
}

/// The breakdown fixture of a display scenario (R8.4, `pob-calc/tests/fixtures/breakdown`): the goldens by slug, the R7-synth as `r81--<slug>`.
fn breakdown_fixture(slug: &str, kind: &str) -> Option<Json> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-calc/tests/fixtures/breakdown");
    let name = if kind == "golden" {
        format!("{slug}.json")
    } else {
        format!("r81--{slug}.json")
    };
    serde_json::from_str(&std::fs::read_to_string(dir.join(name)).ok()?).ok()
}

/// The stat rows of `list` that get a statBox entry with a breakdown key (Build.lua:1913-2005 `AddDisplayStatList`: flags match, the value
/// passes `condFunc` / `~= 0`, not `SkillDPS`, not `hideStat`) -- in order, as `build_display` pushes them.
fn rows_with_breakdown<'s>(list: &'s [DisplayStat], o: &ActorView<'_>, flags: &indexmap::IndexMap<String, bool>) -> Vec<&'s DisplayStat> {
    list.iter()
        .filter(|sd| {
            let Some(stat) = sd.stat else { return false };
            if !match_flags(sd.flag, sd.not_flag, flags) || stat == "SkillDPS" || sd.hide_stat {
                return false;
            }
            let mut v = o.get(stat);
            if v.truthy()
                && let Some(c) = sd.child_stat
            {
                v = v.index(c).expect("childStat");
            }
            if !v.truthy() {
                return false;
            }
            match sd.cond_func {
                Some(c) => c(&v, o).is_ok_and(|r| r.truthy()),
                None => !matches!(v, LuaV::Num(x) if x == 0.0),
            }
        })
        .collect()
}

/// Build.lua:1871-1903 `GetStatBreakdownKey` over Rust's `env.<actor>.breakdown`: `not not key`.
fn rust_underline(bd: &pob_calc::breakdown::Breakdown, sd: &DisplayStat) -> bool {
    use pob_calc::breakdown::{BVal, Breakdown};
    let truthy = |v: Option<&BVal>| !matches!(v, None | Some(BVal::Bool(false)));
    let stat = sd.stat.expect("stat");
    let key: Option<String> = if let Some(b) = sd.breakdown {
        Some(b.to_owned())
    } else if let Some(c) = sd.child_stat {
        match bd.get(stat) {
            Some(BVal::Table(t)) if truthy(t.fields.get(c)) => Some(format!("{stat}.{c}")),
            _ => None,
        }
    } else if truthy(bd.get(stat)) {
        Some(stat.to_owned())
    } else {
        None
    };
    let Some(key) = key else { return false };
    if sd.mod_names.is_some() {
        return true;
    }
    let data = match key.split_once('.') {
        Some((ns, name)) if ns.bytes().all(|b| b.is_ascii_alphabetic()) && name.bytes().all(|b| b.is_ascii_alphabetic()) => {
            match bd.get(ns) {
                Some(BVal::Table(t)) if truthy(t.fields.get(name)) => t.fields.get(name),
                _ => bd.get(&key),
            }
        }
        _ => bd.get(&key),
    };
    data.is_some_and(Breakdown::is_populated)
}

#[derive(Default)]
struct Tally {
    rows: usize,
    raw: usize,
    stat_box: usize,
    warnings: usize,
    unverified_underline: usize,
    /// underline flags decided by the R8.4 breakdown (equal to LuaJIT; a difference is a divergence in `bad`).
    resolved_underline: usize,
    bad: Vec<String>,
}

fn cmp_list(t: &mut Tally, slug: &str, what: &str, rust: &[Json], lua: &[Json]) {
    if rust.len() != lua.len() {
        t.bad
            .push(format!("{slug} | {what} | length rust {} vs luajit {}", rust.len(), lua.len()));
    }
    for (i, (r, l)) in rust.iter().zip(lua).enumerate() {
        if r != l {
            t.bad.push(format!("{slug} | {what}[{}] |\n    rust   {r}\n    luajit {l}", i + 1));
        }
    }
}

fn check(slug: &str, t: &mut Tally) {
    let fx = fixture(slug);
    let res = calc(&fx);
    let st = DisplaySettings::default();
    let disp = build_display(&res, &st).unwrap_or_else(|e| panic!("{slug}: {e}"));

    // (1) the engine_displayStatsJSON structure
    let rust_sections = sections_json(&disp.sections);
    let lua_sections = fx["sections"].as_array().expect("sections");
    let flat = |s: &Json| -> Vec<Json> {
        s.as_array()
            .expect("sections")
            .iter()
            .flat_map(|sec| {
                let head = json!({ "actor": sec["actor"], "title": sec["title"], "slot": sec["slot"] });
                std::iter::once(head).chain(sec["rows"].as_array().expect("rows").iter().cloned())
            })
            .collect()
    };
    t.rows += lua_sections.iter().map(|s| s["rows"].as_array().map_or(0, Vec::len)).sum::<usize>();
    cmp_list(t, slug, "sections(flat)", &flat(&rust_sections), &flat(&fx["sections"]));

    // (2) raw entries (the data + the evaluated functions)
    let minion = MinionBorrow::new(&res);
    let denv = DisplayEnv::new(&res, &minion).expect("display env");
    let raw = |list: &[DisplayStat], view: &ActorView<'_>, flags| -> Vec<Json> {
        list.iter().enumerate().map(|(i, sd)| raw_entry(i + 1, sd, view, flags)).collect()
    };
    let rp = raw(DISPLAY_STATS, &denv.player.view, denv.player.skill_flags);
    t.raw += rp.len();
    cmp_list(t, slug, "rawPlayer", &rp, fx["rawPlayer"].as_array().expect("rawPlayer"));
    match (&denv.minion, fx.get("rawMinion")) {
        (Some(m), Some(Json::Array(l))) => {
            let rm = raw(MINION_DISPLAY_STATS, &m.view, m.skill_flags);
            t.raw += rm.len();
            cmp_list(t, slug, "rawMinion", &rm, l);
        }
        (None, None | Some(Json::Null)) => {}
        (m, l) => t.bad.push(format!(
            "{slug} | rawMinion | rust minion {} vs luajit {:?}",
            m.is_some(),
            l.map(|_| "list")
        )),
    }

    // (3) statBox.list
    let mut rust_box: Vec<Json> = disp.stat_list.stat_box.iter().map(stat_box_json).collect();
    let mut lua_box: Vec<Json> = fx["statBox"].as_array().expect("statBox").clone();
    // R8.4: the stat rows behind the statBox entries with a breakdown key, per actor, and the breakdown classes of the scenario
    let bfx = breakdown_fixture(slug, fx["kind"].as_str().unwrap_or("synth"));
    let mut rows: std::collections::HashMap<&str, (Vec<&DisplayStat>, usize)> = std::collections::HashMap::new();
    rows.insert(
        "player",
        (rows_with_breakdown(DISPLAY_STATS, &denv.player.view, denv.player.skill_flags), 0),
    );
    if let Some(m) = &denv.minion {
        rows.insert("minion", (rows_with_breakdown(MINION_DISPLAY_STATS, &m.view, m.skill_flags), 0));
    }
    let rust_bd = |actor: &str| -> Option<pob_calc::breakdown::Breakdown> {
        match actor {
            "player" => res.perform.player.breakdown.clone(),
            _ => res.perform.minion.as_ref().and_then(|m| m.borrow().breakdown.clone()),
        }
    };
    for (i, (r, e)) in rust_box.iter_mut().zip(disp.stat_list.stat_box.iter()).enumerate() {
        // the stat row of this entry (entries with a breakdown key, in order per actor)
        let sd = match (e.breakdown.as_ref(), e.actor_name.and_then(|a| rows.get_mut(a))) {
            (Some(_), Some((list, at))) => {
                *at += 1;
                list.get(*at - 1).copied()
            }
            _ => None,
        };
        if matches!(e.breakdown, Some(BreakdownKey::Unported))
            && let Some(l) = lua_box.get_mut(i)
            && l["underline"][1].is_boolean()
            && r["underline"][1].is_null()
        {
            // decided when the key's breakdown entry is one R8.4 builds (class `ported`) or no module writes it, the table is the one
            // perform made (not handed over by CalcMirages) -- and the statBox label is this row's
            let actor = e.actor_name.unwrap_or("player");
            let decided = sd
                .filter(|sd| {
                    e.text
                        .as_deref()
                        .is_some_and(|tx| tx.ends_with(&format!("{}:", sd.label.unwrap_or("?"))))
                })
                .and_then(|sd| {
                    let a = bfx.as_ref().map(|b| &b[actor]).filter(|a| a.is_object())?;
                    if a["replaced"] == json!(true) {
                        return None;
                    }
                    let top = sd.breakdown.map_or(sd.stat.expect("stat"), |b| b.split('.').next().unwrap_or(b));
                    let class = a["keys"][top]["class"].as_str();
                    if class.is_some_and(|c| c != "ported") {
                        return None;
                    }
                    Some(rust_underline(&rust_bd(actor)?, sd))
                });
            match decided {
                Some(b) => {
                    r["underline"][1] = Json::Bool(b);
                    t.resolved_underline += 1;
                }
                None => {
                    l["underline"][1] = Json::Null;
                    t.unverified_underline += 1;
                }
            }
        }
    }
    t.stat_box += lua_box.len();
    cmp_list(t, slug, "statBox", &rust_box, &lua_box);

    // (4) warnings (the fixture sorts them: R13) and the rest of the probe
    let mut w = disp.stat_list.warnings.clone();
    let count = w.len();
    w.sort();
    let lw: Vec<String> = fx["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|x| x.as_str().expect("str").to_owned())
        .collect();
    t.warnings += lw.len();
    if w != lw {
        t.bad.push(format!("{slug} | warnings |\n    rust   {w:?}\n    luajit {lw:?}"));
    }
    if num(count as f64) != fx["warningsRawCount"] {
        t.bad.push(format!(
            "{slug} | warningsRawCount | rust {count} vs luajit {}",
            fx["warningsRawCount"]
        ));
    }
    let iw: Map<String, Json> = res.env.item_warnings.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    if Json::Object(iw.clone()) != fx["itemWarnings"] {
        t.bad
            .push(format!("{slug} | itemWarnings | rust {iw:?} vs luajit {}", fx["itemWarnings"]));
    }
    let opt = |s: Option<&str>| s.map_or(Json::Null, |x| json!(x));
    let p = &denv.player;
    for (k, r) in [
        ("infoMessage", opt(p.info_message)),
        ("infoMessage2", opt(p.info_message2)),
        ("disableReason", opt(p.disable_reason)),
        ("mainSkillDisabled", json!(p.skill_flags.get("disable").copied().unwrap_or(false))),
        ("hasMinion", json!(denv.minion.is_some())),
    ] {
        let l = fx.get(k).cloned().unwrap_or(Json::Null);
        if r != l {
            t.bad.push(format!("{slug} | {k} | rust {r} vs luajit {l}"));
        }
    }
}

#[test]
fn display_matches_luajit() {
    let list = scenarios();
    assert_eq!(list.len(), 26, "R8.1 fixtures: 6 goldens + 20 R7-synth");
    let mut total = Tally::default();
    println!("scenario -> section rows / raw entries / statBox entries / warnings / unverified underline / divergences");
    for slug in &list {
        let mut t = Tally::default();
        check(slug, &mut t);
        println!(
            "{slug:<40} {:>4} {:>4} {:>4} {:>2} {:>3} {}",
            t.rows,
            t.raw,
            t.stat_box,
            t.warnings,
            t.unverified_underline,
            t.bad.len()
        );
        total.rows += t.rows;
        total.raw += t.raw;
        total.stat_box += t.stat_box;
        total.warnings += t.warnings;
        total.unverified_underline += t.unverified_underline;
        total.resolved_underline += t.resolved_underline;
        total.bad.extend(t.bad);
    }
    println!(
        "TOTAL {} scenarios: section rows {}, raw entries {}, statBox entries {}, warnings {}, underline flags decided by the R8.4 breakdown {}, UNVERIFIED underline flags {} (R8.5), divergences {}",
        list.len(),
        total.rows,
        total.raw,
        total.stat_box,
        total.warnings,
        total.resolved_underline,
        total.unverified_underline,
        total.bad.len()
    );
    let (pinned, bad): (Vec<&String>, Vec<&String>) = total.bad.iter().partition(|b| {
        KNOWN_MIRAGE_HANDS.iter().any(|(s, k)| b.starts_with(&format!("{s} | {k} |"))) && b.contains("\"valueType\":\"nil\"")
    });
    for b in &bad {
        println!("  {b}");
    }
    println!("pinned (mirage-hands, see KNOWN_MIRAGE_HANDS): {}", pinned.len());
    assert!(bad.is_empty(), "{} divergences", bad.len());
    assert_eq!(
        pinned.len(),
        KNOWN_MIRAGE_HANDS.len(),
        "a pinned divergence went away: update KNOWN_MIRAGE_HANDS"
    );
    assert_eq!(
        total.unverified_underline, UNVERIFIED_UNDERLINE,
        "underline flags Rust cannot decide without the R8.5 breakdown tables"
    );
}

/// Former class "mirage-hands" (R8.3): CalcMirages.lua:157 `env.player.output = newEnv.player.output` hands over the hand tables too;
/// `mirages::replace_main` now copies `ActorState::hands` (orchestrator, R8.3 review) -- no divergence is pinned any more.
const KNOWN_MIRAGE_HANDS: &[(&str, &str)] = &[];

/// The second `underline` flags of stat rows that depend on the contents of `env.<actor>.breakdown` (Build.lua:1881-1902), on the 26
/// scenarios, that Rust cannot decide yet: R8.4 decides 838 of the former 994 (equal to LuaJIT); the 156 left are keys offence / triggers
/// write (class `R8.5` in the breakdown fixtures: ManaCost, ManaPerSecondCost, TotalDPS, PreEffectiveCritChance, Speed, ...).
const UNVERIFIED_UNDERLINE: usize = 156;

/// R8.3 perf: `RefreshStatList` and the display structure on the 6 goldens (median over runs, µs) vs LuaJIT
/// (`tools/.scratch/perf-r8.json`, export-r8.ts `perf`). Run: `cargo test -p pob-engine --release --test display -- --ignored --nocapture`.
#[test]
#[ignore = "perf: run explicitly with --release"]
fn display_perf() {
    const RUNS: usize = 201;
    let perf_path = std::env::var_os("POB_R8_PERF").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/.scratch/perf-r8.json"),
        PathBuf::from,
    );
    let lj: Option<Json> = std::fs::read_to_string(&perf_path).ok().and_then(|s| serde_json::from_str(&s).ok());
    let st = DisplaySettings::default();
    println!("build -> rust refresh / structure (µs, median of {RUNS}) | luajit refreshStatList / displayStructure (µs, median)");
    for slug in scenarios() {
        let fx = fixture(&slug);
        if fx["kind"] != "golden" {
            continue;
        }
        let res = calc(&fx);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for _ in 0..RUNS {
            let minion = MinionBorrow::new(&res);
            let mut denv = DisplayEnv::new(&res, &minion).expect("env");
            let t0 = Instant::now();
            let sl = refresh_stat_list(&mut denv, &st).expect("refresh");
            let t1 = Instant::now();
            let secs = display_sections(&mut denv, &st).expect("sections");
            let t2 = Instant::now();
            std::hint::black_box((sl, secs));
            a.push((t1 - t0).as_secs_f64() * 1e6);
            b.push((t2 - t1).as_secs_f64() * 1e6);
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let name = fx["build"].as_str().expect("build");
        let l = |k: &str| {
            lj.as_ref()
                .and_then(|j| j["builds"][name][k]["median"].as_f64())
                .map_or("n/a".to_owned(), |ms| format!("{:.0}", ms * 1000.0))
        };
        println!(
            "{name:<40} {:>7.1} / {:>7.1} | {:>5} / {:>5}",
            med(&mut a),
            med(&mut b),
            l("refreshStatList"),
            l("displayStructure")
        );
    }
}
