//! R8-real: Rust = LuaJIT on the user's own PoB builds (`builds/pc/`, a local copy, gitignored).
//!
//! Truth: `builds/pc-fixtures/<league>__<file>.luajit.json` (producer `tools/lua-corpus/export-real.ts`, gitignored too -- personal build
//! data): the build as desktop reads it (text mode) -> `loadBuildFromXML` -> `wipeGlobalCache` -> `calcs.buildOutput(build, "MAIN")`; the
//! player's output (`output.{nums,strs,bools}`, numbers `%.17g`) and the minion's (`minion`). A build LuaJIT could not load / calculate
//! (`loadError` / `calcError`) is listed, not gated. Without `builds/pc` or the fixtures the gate is skipped with a message (other machines).
//!
//! `real_builds_match_luajit`: `calculate_file(builds/pc/<rel>, Full, skills: true)`; EVERY scalar key of the player's output and of
//! `Minion.*` bit for bit; a key LuaJIT has and Rust lacks is "missing", the reverse "extra". A stale fixture (the build file changed since
//! the probe) fails. `real_builds_perf` (ignored; `--release`): `CalcSession::build_output(MAIN)` after `wipe_global_cache`, median of 9
//! after one warm-up, vs the LuaJIT medians of `_perf-luajit.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::{CalculateOptions, ParseMode, StatValue, calculate_file};
use pob_golden::compare::stats_exact;
use pob_golden::value::StatValue as Gv;
use pob_lua::format_g;
use serde_json::Value as Json;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../builds")
}

fn fixtures_dir() -> PathBuf {
    root().join("pc-fixtures")
}

fn lua_num(s: &str) -> Gv {
    match s {
        "nan" | "-nan" | "inf" | "-inf" => Gv::Str(s.to_owned()),
        _ => Gv::Num(s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}"))),
    }
}

fn to_golden(v: &StatValue) -> Gv {
    match v {
        StatValue::Num(n) => Gv::Num(*n),
        StatValue::Str(s) => Gv::Str(s.clone()),
        StatValue::Bool(b) => Gv::Bool(*b),
    }
}

fn show(v: &Gv) -> String {
    match v {
        Gv::Num(n) => format_g(*n, 17),
        Gv::Str(s) => format!("{s:?}"),
        Gv::Bool(b) => b.to_string(),
    }
}

/// The scalar part of a LuaJIT output dump (`nums`/`strs`/`bools`; an empty Lua table is `[]` in dkjson).
fn scalars(dump: &Json) -> BTreeMap<String, Gv> {
    let empty = serde_json::Map::new();
    let obj = |k: &str| dump[k].as_object().unwrap_or(&empty);
    let mut m = BTreeMap::new();
    for (k, v) in obj("nums") {
        m.insert(k.clone(), lua_num(v.as_str().expect("num string")));
    }
    for (k, v) in obj("strs") {
        m.insert(k.clone(), Gv::Str(v.as_str().expect("str").to_owned()));
    }
    for (k, v) in obj("bools") {
        m.insert(k.clone(), Gv::Bool(v.as_bool().expect("bool")));
    }
    m
}

/// `(slug, fixture)` of every fixture, sorted; `None` when the corpus is absent on this machine.
fn corpus() -> Option<Vec<(String, Json)>> {
    if !root().join("pc").is_dir() || !fixtures_dir().is_dir() {
        return None;
    }
    let mut v: Vec<(String, Json)> = std::fs::read_dir(fixtures_dir())
        .ok()?
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let slug = name.strip_suffix(".luajit.json")?.to_owned();
            let text = std::fs::read_to_string(fixtures_dir().join(&name)).expect("fixture");
            Some((slug, serde_json::from_str(&text).expect("fixture json")))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    (!v.is_empty()).then_some(v)
}

fn sha256_hex(bytes: &[u8]) -> String {
    // tiny SHA-256 (no new crates): the fixture's `xmlSha256` of the desktop-text-mode bytes
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = bytes.to_vec();
    let bits = (bytes.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, c) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// The build bytes as desktop reads them (text mode: CRLF -> LF), what the probe hashed.
fn desktop_bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).expect("build file");
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\r' && raw.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(raw[i]);
        i += 1;
    }
    out
}

#[derive(Default)]
struct Row {
    slug: String,
    compared: usize,
    exact: usize,
    missing: usize,
    extra: usize,
    minion_compared: usize,
    minion_exact: usize,
    bad: Vec<String>,
    unimplemented: Vec<String>,
    /// [`p5_inputs`] of the build file.
    p5: Vec<String>,
    /// [`boundary_inputs`] of the build file.
    boundary: Vec<String>,
    /// The fixture's `xmlSha256` (what [`KNOWN`] pins -- no personal build names in the repo).
    sha: String,
}

fn compare(name: &str, prefix: &str, lua: &BTreeMap<String, Gv>, rust: &BTreeMap<String, Gv>, row: &mut Row, minion: bool) {
    for (k, l) in lua {
        let (c, e) = if minion {
            (&mut row.minion_compared, &mut row.minion_exact)
        } else {
            (&mut row.compared, &mut row.exact)
        };
        *c += 1;
        match rust.get(k) {
            Some(r) if stats_exact(l, r) => *e += 1,
            Some(r) => row
                .bad
                .push(format!("{name} | {prefix}{k} | rust {} | luajit {} | MISMATCH", show(r), show(l))),
            None => {
                row.missing += 1;
                row.bad
                    .push(format!("{name} | {prefix}{k} | rust nil | luajit {} | MISSING", show(l)));
            }
        }
    }
    for (k, r) in rust {
        if !lua.contains_key(k) {
            row.extra += 1;
            row.bad
                .push(format!("{name} | {prefix}{k} | rust {} | luajit nil | EXTRA", show(r)));
        }
    }
}

/// The build file a fixture was made from: the user's build, or (variant `p5stripped`) the probe's copy without the P5 inputs.
fn build_path(fx: &Json) -> PathBuf {
    match fx["xmlFile"].as_str() {
        Some(f) => fixtures_dir().join(f),
        None => root().join("pc").join(fx["build"].as_str().expect("build")),
    }
}

/// The inputs of the two P5 modules pob-calc does not port that this build really uses (measured from its XML): tattoo `<Override>`s
/// (PassiveSpec.lua:158-186 -> `hashOverrides` -> ReplaceNode :1212-1220) and imported party buffs (`<Party><ImportedBuffs>` with content
/// -> PartyTab -> `env.partyMembers` CalcPerform.lua:1310, `enemyModList` CalcSetup.lua:685).
fn p5_inputs(xml: &str) -> Vec<String> {
    let mut v = Vec::new();
    let tattoos = xml.matches("<Override ").count();
    if tattoos > 0 {
        v.push(format!("P5 tattoo overrides ({tattoos})"));
    }
    let mut party = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<ImportedBuffs name=\"") {
        rest = &rest[i + 21..];
        let name = rest.split('"').next().unwrap_or("");
        let body_start = rest.find('>').map_or(rest.len(), |j| j + 1);
        let self_closing = rest[..body_start].ends_with("/>");
        let body_end = rest.find("</ImportedBuffs>").unwrap_or(body_start);
        if !self_closing && body_end > body_start && !rest[body_start..body_end].trim().is_empty() {
            party.push(name.to_owned());
        }
    }
    if !party.is_empty() {
        v.push(format!("P5 party buffs ({})", party.join(",")));
    }
    v
}

fn run_build(slug: &str, fx: &Json) -> Row {
    let rel = fx["build"].as_str().expect("build");
    let path = build_path(fx);
    let mut row = Row {
        slug: slug.to_owned(),
        ..Row::default()
    };
    let bytes = desktop_bytes(&path);
    row.p5 = p5_inputs(&String::from_utf8_lossy(&bytes));
    row.boundary = boundary_inputs(&String::from_utf8_lossy(&bytes));
    let sha = sha256_hex(&bytes);
    row.sha.clone_from(&sha);
    if fx["xmlSha256"].as_str() != Some(sha.as_str()) {
        row.bad.push(format!(
            "{slug} | <fixture> | stale: {rel} changed since the probe -- re-run export-real.ts"
        ));
        return row;
    }
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| calculate_file(&path, opts)));
    let res = match outcome {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            row.bad.push(format!("{slug} | <whole build> | rust Err: {e}"));
            return row;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            row.bad.push(format!("{slug} | <whole build> | rust PANIC: {msg}"));
            return row;
        }
    };
    row.unimplemented.clone_from(&res.unimplemented);
    let rust: BTreeMap<String, Gv> = res.stats.iter().map(|(k, v)| (k.clone(), to_golden(v))).collect();
    compare(slug, "", &scalars(&fx["output"]), &rust, &mut row, false);
    let lua_minion = fx["minion"].as_object().map(|_| scalars(&fx["minion"]));
    let rust_minion = res.perform.minion.as_ref().map(|m| {
        pob_calc::output_to_stats(&m.borrow().output)
            .iter()
            .map(|(k, v)| (k.clone(), to_golden(v)))
            .collect::<BTreeMap<_, _>>()
    });
    match (lua_minion, rust_minion) {
        (Some(l), Some(r)) => compare(slug, "Minion.", &l, &r, &mut row, true),
        (None, None) => {}
        (l, r) => row
            .bad
            .push(format!("{slug} | Minion | luajit env.minion {} rust {}", l.is_some(), r.is_some())),
    }
    row
}

/// `unimplemented` notes every build carries (the 6 goldens too: party / buff export, display, UI-only, P5 tree bookkeeping that does not
/// change the output) -- the rest are what this build reaches beyond them.
const EVERY_BUILD_NOTES: [&str; 11] = [
    "CalcPerform.lua:1312 ",
    "CalcPerform.lua:3803-3990 ",
    "CalcSetup.lua:1011 ",
    "CalcSetup.lua:174 ",
    "CalcSetup.lua:685 ",
    "Calcs.lua:450-464 ",
    "PassiveSpec.lua:1212-1220 ",
    "PassiveSpec.lua:1520-1560 ",
    "PassiveSpec.lua:1591-1617 ",
    "PassiveSpec.lua:196 ",
    "PassiveSpec.lua:630-663 ",
];

fn boundary_notes(notes: &[String]) -> Vec<String> {
    let mut v: Vec<String> = notes
        .iter()
        .filter(|u| !EVERY_BUILD_NOTES.iter().any(|p| u.starts_with(p)))
        .map(|u| u.chars().take(60).collect())
        .collect();
    v.dedup();
    v
}

/// Own `unimplemented` notes that mark a port boundary the build reaches (the port records the skipped upstream branch; reported, not gated):
/// Split Personality's `distanceToClassStart` (P5 path machinery), `LinkedSupport` without `mod.sourceSlot`, the Energy Blade initEnv re-run /
/// weapon data, `ExtraJewelFunc` (initEnv recursion).
const BOUNDARY_NOTES: [&str; 5] = [
    "CalcSetup.lua:1061 ",
    "CalcSetup.lua:1595 ",
    "CalcSetup.lua:1923 ",
    "Modules/CalcOffence.lua:568-583 ",
    "CalcSetup.lua:1434 ",
];

/// Port boundaries read from the build file: a legacy alternate ascendancy (`secondaryAscendClassId` 1-3 = Warden / Warlock / Primalist) --
/// PassiveTree.lua:132-176 removes their nodes from every tree that lists them (pob-tree: `PassiveTree.lua:131-172 alternate_ascendancies`,
/// P5), Rust keeps and allocates them (measured: 9 extra allocated nodes, charm sockets with charms).
fn boundary_inputs(xml: &str) -> Vec<String> {
    let mut v = Vec::new();
    for id in ["1", "2", "3"] {
        if xml.contains(&format!("secondaryAscendClassId=\"{id}\"")) {
            v.push(format!("legacy alternate ascendancy {id} (PassiveTree.lua:132-176, pob-tree P5)"));
        }
    }
    v
}

/// Divergences measured to the root but outside this card's files, pinned by the build's sha256 prefix (no names in the repo) and the
/// exact number of divergent (build, key) pairs: `(sha prefix, pairs, cause)`.
const KNOWN: [(&str, usize, &str); 4] = [
    ("eadab62bb90e601b", 31, CLUSTER_SOCKET),
    ("ceeb2e12d6ac6841", 22, CLUSTER_SOCKET),
    ("41ed0be8d17e4d48", 29, CLUSTER_SOCKET),
    (
        "6eee1e8401984f67",
        1,
        "ManaFlaskChargeGen 1-2 ulp (2.6666666666666701 vs ...696): root not measured",
    ),
];

/// A plain jewel saved in a socket of a cluster subgraph under its legacy node id (`<Socket nodeId="11150">`, a `Small Jewel Socket` with
/// `expansionJewel.parent`): LuaJIT's spec has no such node (PassiveSpec.lua:54) and clears `spec.jewels[11150]` to 0 -- the jewel is not
/// applied; Rust remaps the id through the legacy cluster-hash conversion (pob-tree `cluster.rs`, 11150 -> 6910, 16218 -> 29712) and applies it.
const CLUSTER_SOCKET: &str = "jewel in a legacy cluster-subgraph socket: LuaJIT drops it, Rust remaps it (pob-tree cluster.rs)";

/// Per-build limit of the Rust gate (a broken build that loads for minutes is reported as a timeout, not waited for).
const BUILD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[test]
fn real_builds_match_luajit() {
    let Some(corpus) = corpus() else {
        println!("real_builds: builds/pc or builds/pc-fixtures absent -- skipped (personal corpus, local only)");
        return;
    };
    let mut not_loaded = Vec::new();
    let mut timeouts: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for (slug, fx) in &corpus {
        if let Some(e) = fx["loadError"].as_str().or(fx["calcError"].as_str()).or(fx["timeout"].as_str()) {
            not_loaded.push(format!("{slug}: {e}"));
            continue;
        }
        // a broken build may take very long: each build runs on its own thread, abandoned after BUILD_TIMEOUT (reported, gated)
        let (tx, rx) = std::sync::mpsc::channel();
        let (s, f) = (slug.clone(), fx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(run_build(&s, &f));
        });
        match rx.recv_timeout(BUILD_TIMEOUT) {
            Ok(r) => rows.push(r),
            Err(_) => timeouts.push(slug.clone()),
        }
    }
    println!("--- timeout (> {BUILD_TIMEOUT:?} in Rust): {timeouts:?} ---");
    println!(
        "{:<56} {:>8} {:>8} {:>6} {:>7} {:>5} {:>6} {:>7}  P5 inputs | own unimplemented notes",
        "build", "compared", "exact", "bad", "missing", "extra", "minion", "m.exact"
    );
    // class of a row: "equal"; "p5" = as-is build that uses a P5 input (reported, the p5stripped variant is its gate); "bad" = gated divergence
    let class = |r: &Row| {
        let own_boundary = r.unimplemented.iter().any(|u| BOUNDARY_NOTES.iter().any(|p| u.starts_with(p)));
        if r.bad.is_empty() {
            "equal"
        } else if !r.slug.ends_with(".p5stripped") && !r.p5.is_empty() {
            "p5"
        } else if !r.boundary.is_empty() || own_boundary {
            "boundary"
        } else if KNOWN.iter().any(|(sha, n, _)| r.sha.starts_with(sha) && *n == r.bad.len()) {
            "known"
        } else {
            "bad"
        }
    };
    let mut by_league: BTreeMap<String, [usize; 6]> = BTreeMap::new();
    for r in &rows {
        println!(
            "{:<56} {:>8} {:>8} {:>6} {:>7} {:>5} {:>6} {:>7}  {} | {}",
            r.slug,
            r.compared,
            r.exact,
            r.bad.len(),
            r.missing,
            r.extra,
            r.minion_compared,
            r.minion_exact,
            [r.p5.clone(), r.boundary.clone()].concat().join(", "),
            boundary_notes(&r.unimplemented).join(" ; ")
        );
        let league = r.slug.split("__").next().unwrap_or("?");
        let key = if r.slug.ends_with(".p5stripped") {
            format!("{league} [p5stripped]")
        } else {
            league.to_owned()
        };
        let e = by_league.entry(key).or_default();
        e[0] += 1;
        e[match class(r) {
            "equal" => 1,
            "p5" => 2,
            "boundary" => 3,
            "known" => 4,
            _ => 5,
        }] += 1;
    }
    println!(
        "--- per league: builds / = LuaJIT / P5 inputs stripped by the probe (tattoos, party) / other port boundary / known (pinned) / other divergence ---"
    );
    for (l, [n, eq, p5, bd, kn, bad]) in &by_league {
        println!("{l:<24} {n:>3} {eq:>3} {p5:>3} {bd:>3} {kn:>3} {bad:>3}");
    }
    println!("--- not loaded / not calculated in LuaJIT ({}) ---", not_loaded.len());
    for n in &not_loaded {
        println!("{n}");
    }
    println!("--- every divergence (class: build | key | rust | luajit) ---");
    for r in &rows {
        for b in &r.bad {
            println!("{}: {b}", class(r));
        }
    }
    let n = |c: &str| rows.iter().filter(|r| class(r) == c).count();
    let pairs = |c: &str| rows.iter().filter(|r| class(r) == c).map(|r| r.bad.len()).sum::<usize>();
    println!(
        "real_builds: {} fixtures; {} = LuaJIT bit for bit; {} P5 inputs ({} pairs); {} other boundary ({} pairs); {} known ({} pairs); {} other divergent ({} pairs); {} player + {} Minion keys compared; {} not loaded in LuaJIT",
        rows.len(),
        n("equal"),
        n("p5"),
        pairs("p5"),
        n("boundary"),
        pairs("boundary"),
        n("known"),
        pairs("known"),
        n("bad"),
        pairs("bad"),
        rows.iter().map(|r| r.compared).sum::<usize>(),
        rows.iter().map(|r| r.minion_compared).sum::<usize>(),
        not_loaded.len()
    );
    // every as-is build at the P5 boundary has its p5stripped twin in the corpus (the twin is gated)
    for r in rows.iter().filter(|r| class(r) == "p5") {
        let twin = format!("{}.p5stripped", r.slug);
        assert!(
            rows.iter().any(|t| t.slug == twin),
            "{}: at the P5 boundary but no {twin} fixture (re-run export-real.ts)",
            r.slug
        );
    }
    assert!(timeouts.is_empty(), "builds over {BUILD_TIMEOUT:?} in Rust: {timeouts:?}");
    assert_eq!(
        n("known"),
        KNOWN.len(),
        "a pinned known divergence changed or vanished -- update KNOWN"
    );
    assert_eq!(
        pairs("bad"),
        0,
        "{} (build, key) pairs differ from LuaJIT outside the recorded boundaries, see the report above",
        pairs("bad")
    );
}

/// Prints Rust's `unimplemented` notes per build (the branches a real build reaches that the port skips knowingly).
#[test]
fn real_builds_unimplemented_notes() {
    let Some(corpus) = corpus() else {
        println!("real_builds: corpus absent -- skipped");
        return;
    };
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for (slug, fx) in &corpus {
        if fx["loadError"].is_string() || fx["calcError"].is_string() {
            continue;
        }
        let r = run_build(slug, fx);
        for u in r.unimplemented {
            *count.entry(u).or_default() += 1;
        }
    }
    for (u, n) in &count {
        println!("{n:>4}  {u}");
    }
}

#[test]
#[ignore = "perf: cargo test --release -p pob-calc --test real_builds -- --ignored --nocapture"]
fn real_builds_perf() {
    let Some(corpus) = corpus() else {
        println!("real_builds: corpus absent -- skipped");
        return;
    };
    let lj: Json = std::fs::read_to_string(fixtures_dir().join("_perf-luajit.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Json::Null);
    const RUNS: usize = 9;
    println!("build -> rust buildOutput(MAIN) ms (median of {RUNS}) | luajit ms (median) | ratio");
    let mut ratios = Vec::new();
    for (slug, fx) in &corpus {
        let Some(l) = lj["builds"][slug.as_str()]["median"].as_f64() else {
            continue;
        };
        if fx["loadError"].is_string() {
            continue;
        }
        let path = root().join("pc").join(fx["build"].as_str().expect("build"));
        let xml = pob_build::read_build_file(&path).expect("xml");
        let model = std::rc::Rc::new(pob_build::load_build_xml(&xml).expect("xml parses"));
        let opts = CalculateOptions {
            parse_mode: ParseMode::Full,
            skills: true,
            ..CalculateOptions::default()
        };
        let sess = pob_calc::CalcSession::new(model, opts);
        let mut t = Vec::new();
        for i in 0..=RUNS {
            sess.wipe_global_cache();
            let t0 = std::time::Instant::now();
            let out = sess.build_output(pob_calc::CalcMode::Main).expect("buildOutput");
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            drop(out);
            if i > 0 {
                t.push(ms);
            }
        }
        t.sort_by(f64::total_cmp);
        let r = t[RUNS / 2];
        ratios.push(l / r);
        println!("{slug:<52} {r:>9.2} | {l:>9.2} | x{:.2}", l / r);
    }
    ratios.sort_by(f64::total_cmp);
    if !ratios.is_empty() {
        println!(
            "LuaJIT / Rust: median x{:.2}, min x{:.2}, max x{:.2} over {} builds",
            ratios[ratios.len() / 2],
            ratios[0],
            ratios[ratios.len() - 1],
            ratios.len()
        );
    }
}
