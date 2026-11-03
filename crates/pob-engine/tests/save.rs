//! R8.8 gate: `SaveDB` (Build.lua:2277 + every tab's `Save`) = LuaJIT, and `load(save(b)) == b`.
//!
//! Fixtures `tests/fixtures/r8/<slug>.save.{xml,json}` (producer `tools/lua-corpus/export-r8.ts`, R8.1): LuaJIT's `build:SaveDB("code")` after
//! `loadBuildFromXML` + `wipeGlobalCache` + `calcsTab:BuildOutput()` + `RefreshStatList` + the display probe (its in-place `SkillDPS` sort),
//! in the CANONICAL form of R8.1 (`save.json.rule`; R13: attribute order, section order, ConfigSet/Calcs `Input`, `Placeholder`, ItemSet
//! `Slot`/`SocketIdURL`; the integer-keyed `pairs` of a spec -- `nodes`, `masteryEffects`, `Sockets`, `Overrides` -- sorted too; an R13
//! ambiguous gem variant -> `<R13-ambiguous>` with the candidates in `r13Gems`). The Rust XML goes through the same canonical form
//! ([`canon`], a port of export-r8.ts `canon`/`compose`) and is compared byte for byte; the R13 parts separately: the Rust gem variant must
//! be one of LuaJIT's candidates, and the items with 2+ base lines (Item.lua:1951/1981, R8.2b) compare with each base-line run sorted.
//!
//! Round trip: the Rust `SaveDB` XML loaded again gives the same output (every scalar of `mainOutput`, bit for bit) and the same XML.

use std::path::{Path, PathBuf};
use std::time::Instant;

use pob_calc::pob_build::{XmlElement, XmlNode};
use pob_engine::engine::Engine;
use serde_json::Value as Json;

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
                .strip_suffix(".save.json")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn xml_of(fx: &Json) -> String {
    let name = fx["build"].as_str().expect("build");
    let path = match fx["kind"].as_str() {
        Some("golden") => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
        _ => synth_dir().join(format!("{name}.xml")),
    };
    read(&path)
}

// --- export-r8.ts `canon` + `compose` ----------------------------------------------------------------------------------------------

fn encode(s: &str) -> String {
    let mut o = String::new();
    pob_calc::pob_build::xml::encode_content(s, &mut o);
    o
}

/// An ambiguous gem: `(skillId, Rust variantId)`.
type R13Gem = (String, String);

fn rank(parent: &str, child: &str) -> Option<&'static str> {
    match (parent, child) {
        ("ConfigSet", "Input") | ("Calcs", "Input") | ("ItemSet", "Slot" | "SocketIdURL") => Some("1"),
        ("ConfigSet", "Placeholder") => Some("2"),
        _ => None,
    }
}

fn canon_node(n: &XmlElement, r13: &mut Vec<R13Gem>) -> XmlElement {
    let mut n = n.clone();
    if n.name == "Gem"
        && let (Some(gid), Some(sid)) = (n.attr("gemId").map(str::to_owned), n.attr("skillId").map(str::to_owned))
        && let Some(vs) = pob_data::gems::gems().gems_by_game_id.0.get(&gid)
    {
        let cands =
            vs.0.values()
                .filter(|id| pob_data::gems::gems().gems.0[*id].granted_effect_id == sid)
                .count();
        if cands > 1 {
            r13.push((sid, n.attr("variantId").unwrap_or("").to_owned()));
            n.attrib.insert("nameSpec".into(), "<R13-ambiguous>".into());
            n.attrib.insert("variantId".into(), "<R13-ambiguous>".into());
        }
    }
    if n.name == "Spec" {
        let mut ids: Vec<u64> = n
            .attr("nodes")
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().expect("id"))
            .collect();
        ids.sort_unstable();
        n.attrib
            .insert("nodes".into(), ids.iter().map(u64::to_string).collect::<Vec<_>>().join(","));
        let mut ms: Vec<String> = n.attr("masteryEffects").unwrap_or("").split("},").map(str::to_owned).collect();
        ms.retain(|s| !s.is_empty());
        let mut ms: Vec<String> = ms
            .into_iter()
            .map(|s| if s.ends_with('}') { s } else { format!("{s}}}") })
            .collect();
        ms.sort();
        n.attrib.insert("masteryEffects".into(), ms.join(","));
    }
    if n.name == "Sockets" || n.name == "Overrides" {
        let key = |c: &XmlNode| match c {
            XmlNode::Element(e) => e.attr("nodeId").and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0),
            XmlNode::Text(_) => 0.0,
        };
        n.children.sort_by(|a, b| key(a).total_cmp(&key(b)));
    }
    let mut kids: Vec<XmlNode> = n
        .children
        .iter()
        .map(|c| match c {
            XmlNode::Element(e) => XmlNode::Element(canon_node(e, r13)),
            t => t.clone(),
        })
        .collect();
    let pos: Vec<usize> = (0..kids.len())
        .filter(|&i| matches!(&kids[i], XmlNode::Element(e) if rank(&n.name, &e.name).is_some()))
        .collect();
    if !pos.is_empty() {
        let mut grp: Vec<XmlNode> = pos.iter().map(|&i| kids[i].clone()).collect();
        let k = |x: &XmlNode| match x {
            XmlNode::Element(e) => format!("{}\0{}", rank(&n.name, &e.name).unwrap_or(""), e.attr("name").unwrap_or("")),
            XmlNode::Text(_) => String::new(),
        };
        grp.sort_by_key(k);
        for (j, &i) in pos.iter().enumerate() {
            kids[i] = grp[j].clone();
        }
    }
    n.children = kids;
    n
}

fn compose(n: &XmlElement, lvl: usize, out: &mut String) {
    out.push_str(&"\t".repeat(lvl));
    out.push('<');
    out.push_str(&n.name);
    let mut keys: Vec<&String> = n.attrib.keys().collect();
    keys.sort();
    for k in keys {
        out.push_str(&format!(" {k}=\"{}\"", encode(&n.attrib[k])));
    }
    if n.children.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for c in &n.children {
        match c {
            XmlNode::Element(e) => compose(e, lvl + 1, out),
            XmlNode::Text(t) => {
                out.push_str(&"\t".repeat(lvl + 1));
                out.push_str(&encode(t));
                out.push('\n');
            }
        }
    }
    out.push_str(&"\t".repeat(lvl));
    out.push_str(&format!("</{}>\n", n.name));
}

fn canon(root: &XmlElement, r13: &mut Vec<R13Gem>) -> String {
    let mut c = canon_node(root, r13);
    let first = c.children.remove(0);
    c.children.sort_by(|a, b| match (a, b) {
        (XmlNode::Element(x), XmlNode::Element(y)) => x.name.cmp(&y.name),
        _ => std::cmp::Ordering::Equal,
    });
    c.children.insert(0, first);
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    compose(&c, 0, &mut out);
    out
}

/// The canonical XML of the Rust `SaveDB` of `e`.
fn rust_canon(e: &Engine, r13: &mut Vec<R13Gem>) -> String {
    let tree = pob_engine::save::save_db(&pob_engine::save::SaveInput {
        model: e.model(),
        extras: e.extras(),
        res: e.result(),
        skill_dps: e.skill_dps(),
        calcs_env: Some(&e.calcs_run().env),
        skills_live: None,
    })
    .unwrap_or_else(|err| panic!("SaveDB: {err}"));
    canon(&tree, r13)
}

/// The `<Item>` texts by id of a canonical XML (lines from `\t\t<Item id="n"` to `\t\t</Item>`).
fn item_blocks(x: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = x.lines().collect();
    let mut v = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("\t\t<Item ") {
            let s = i;
            while !lines[i].starts_with("\t\t</Item>") {
                i += 1;
            }
            v.push((s, i));
        }
        i += 1;
    }
    v
}

/// R8.2b canonical text of one item block: every maximal run of base lines (a `base_lines` key after its `{..}` tags) sorted.
fn sort_base_runs(lines: &[&str], keys: &[String]) -> Vec<String> {
    let strip = |l: &str| -> String {
        let mut s = l.trim_start_matches('\t');
        while s.starts_with('{') {
            match s.find('}') {
                Some(p) => s = &s[p + 1..],
                None => break,
            }
        }
        s.to_owned()
    };
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if keys.contains(&strip(lines[i])) {
            let s = i;
            while i < lines.len() && keys.contains(&strip(lines[i])) {
                i += 1;
            }
            let mut run: Vec<String> = lines[s..i].iter().map(|l| (*l).to_owned()).collect();
            run.sort();
            out.extend(run);
        } else {
            out.push(lines[i].to_owned());
            i += 1;
        }
    }
    out
}

struct Cmp {
    equal: bool,
    first_diff: Option<String>,
    r13_items: usize,
}

/// Rust canonical vs fixture canonical; an item block that differs is compared again with its base-line runs sorted (R13).
fn compare(rust: &str, lj: &str, e: &Engine) -> Cmp {
    if rust == lj {
        return Cmp {
            equal: true,
            first_diff: None,
            r13_items: 0,
        };
    }
    let (rl, ll): (Vec<&str>, Vec<&str>) = (rust.lines().collect(), lj.lines().collect());
    let (rb, lb) = (item_blocks(rust), item_blocks(lj));
    let mut r13_items = 0;
    let mut r_out: Vec<String> = Vec::new();
    let mut l_out: Vec<String> = Vec::new();
    let (mut ri, mut li) = (0, 0);
    let items: Vec<_> = e.result().env.items.values().collect();
    for (k, (&(rs, re), &(ls, le))) in rb.iter().zip(lb.iter()).enumerate() {
        r_out.extend(rl[ri..rs].iter().map(|s| (*s).to_owned()));
        l_out.extend(ll[li..ls].iter().map(|s| (*s).to_owned()));
        let (rr, lr) = (&rl[rs..=re], &ll[ls..=le]);
        if rr != lr {
            let keys: Vec<String> = items.get(k).map(|i| i.base_lines.keys().cloned().collect()).unwrap_or_default();
            let (a, b) = (sort_base_runs(rr, &keys), sort_base_runs(lr, &keys));
            if a == b && keys.len() >= 2 {
                r13_items += 1;
            }
            r_out.extend(a);
            l_out.extend(b);
        } else {
            r_out.extend(rr.iter().map(|s| (*s).to_owned()));
            l_out.extend(lr.iter().map(|s| (*s).to_owned()));
        }
        ri = re + 1;
        li = le + 1;
    }
    r_out.extend(rl[ri..].iter().map(|s| (*s).to_owned()));
    l_out.extend(ll[li..].iter().map(|s| (*s).to_owned()));
    let equal = r_out == l_out;
    let first_diff = (!equal).then(|| {
        let i = r_out
            .iter()
            .zip(l_out.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(r_out.len().min(l_out.len()));
        let cut = |s: Option<&String>| s.map_or("<end>".to_owned(), |s| s.chars().take(300).collect());
        format!(
            "line {}: rust {:?}\n          luajit {:?}",
            i + 1,
            cut(r_out.get(i)),
            cut(l_out.get(i))
        )
    });
    Cmp {
        equal,
        first_diff,
        r13_items,
    }
}

fn stats_debug(e: &Engine) -> Vec<(String, String)> {
    e.result().stats.iter().map(|(k, v)| (k.clone(), format!("{v:?}"))).collect()
}

#[test]
fn save_db_matches_luajit_and_round_trips() {
    let mut bad = Vec::new();
    let (mut ok, mut rt_ok, mut r13_items, mut r13_gems) = (0, 0, 0, 0);
    let (mut t_load, mut t_save) = (0.0, 0.0);
    let (mut xml_first_pass, mut url_cycle) = (0, 0);
    let names = scenarios();
    for slug in &names {
        let fx: Json = serde_json::from_str(&read(&dir().join(format!("{slug}.save.json")))).expect("json");
        let lj = read(&dir().join(format!("{slug}.save.xml")));
        let xml = xml_of(&fx);
        let t0 = Instant::now();
        let mut e = Engine::load_xml(&xml).unwrap_or_else(|err| panic!("{slug}: {err}"));
        t_load += t0.elapsed().as_secs_f64() * 1e3;
        // the probe ran the display JSON (a second in-place sort of SkillDPS) before SaveDB
        e.display_stats_json().unwrap_or_else(|err| panic!("{slug}: display: {err}"));
        let mut r13 = Vec::new();
        let t1 = Instant::now();
        let rust = rust_canon(&e, &mut r13);
        t_save += t1.elapsed().as_secs_f64() * 1e3;
        // R13 gems: the Rust variant is one of LuaJIT's candidates
        let cands = fx["r13Gems"].as_array().expect("r13Gems");
        for (sid, vid) in &r13 {
            let found = cands.iter().any(|c| {
                c["skillId"] == sid.as_str()
                    && c["candidates"]
                        .as_array()
                        .is_some_and(|l| l.iter().any(|x| x.as_str().is_some_and(|x| x.starts_with(&format!("{vid}=")))))
            });
            if !found {
                bad.push(format!("{slug}: R13 gem {sid} variant {vid} not among LuaJIT's candidates"));
            }
            r13_gems += 1;
        }
        let c = compare(&rust, &lj, &e);
        r13_items += c.r13_items;
        if c.equal {
            ok += 1;
        } else {
            bad.push(format!("{slug}: SaveDB != LuaJIT\n    {}", c.first_diff.unwrap_or_default()));
        }
        // load(save(b)) == b
        let saved = e.save_xml().unwrap_or_else(|err| panic!("{slug}: {err}"));
        let e2 = Engine::load_xml(&saved).unwrap_or_else(|err| panic!("{slug}: reload: {err}"));
        let (a, b) = (stats_debug(&e), stats_debug(&e2));
        let mut e2 = e2;
        e2.display_stats_json().expect("display");
        let rust2 = rust_canon(&e2, &mut Vec::new());
        // The XML is not a fixed point of upstream's own load+save on the first pass (load normalisations: `includeInFullDPS="nil"` ->
        // false (SkillsTab.lua:447), `mainActiveSkill="nil"` -> 1 (:450), `<TimelessData>` -> `socketFilterDistance = 0` (Build.lua:1010),
        // the `nodes` order of the reloaded spec -> another `pairs(allocNodes)` order of the URL (R11)) -- counted, then the second
        // pass must be stable.
        let first_pass = rust.lines().zip(rust2.lines()).filter(|(x, y)| x != y).count();
        xml_first_pass += first_pass;
        let saved2 = e2.save_xml().unwrap_or_else(|err| panic!("{slug}: {err}"));
        let mut e3 = Engine::load_xml(&saved2).unwrap_or_else(|err| panic!("{slug}: reload 2: {err}"));
        e3.display_stats_json().expect("display");
        let c3 = stats_debug(&e3);
        let rust3 = rust_canon(&e3, &mut Vec::new());
        let url = |x: &str| -> String {
            x.lines().filter(|l| !l.contains("passive-skill-tree/")).collect::<Vec<_>>().join(
                "
",
            )
        };
        if a == b && b == c3 && url(&rust3) == url(&rust2) {
            rt_ok += 1;
            url_cycle += usize::from(rust3 != rust2);
        } else {
            let d = a.iter().zip(b.iter()).find(|(x, y)| x != y).map(|(x, y)| format!("{x:?} vs {y:?}"));
            let xd = compare(&rust3, &rust2, &e).first_diff;
            bad.push(format!(
                "{slug}: round trip: output equal {} / {} ({} vs {} keys, first {d:?}), second-pass XML equal {} {}",
                a == b,
                b == c3,
                a.len(),
                b.len(),
                rust3 == rust2,
                xd.unwrap_or_default()
            ));
        }
    }
    println!(
        "SaveDB = LuaJIT {ok}/{}; round trip {rt_ok}/{}; R13 items (base lines sorted) {r13_items}; R13 gems {r13_gems}; XML lines changed by upstream load normalisations on the first pass {xml_first_pass}, URL order still moving on the second {url_cycle}; load {t_load:.0} ms, SaveDB {t_save:.0} ms (all scenarios, debug or release)",
        names.len(),
        names.len()
    );
    assert!(bad.is_empty(), "{} failures:\n{}", bad.len(), bad.join("\n"));
    assert_eq!(names.len(), 26);
}
