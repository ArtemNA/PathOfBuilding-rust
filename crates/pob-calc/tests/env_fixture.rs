//! R3.6 gate (R3-ARCH §3) on the R3.1 fixtures (`tests/fixtures/<build>.{luajit,pobts}.json`, key `env`; producer
//! `tools/lua-corpus/export-env.ts`, probe = pob-ts `test/calc/initEnv-oracle.test.ts:88-116`):
//!   (a) the Rust dump in the shape of the probe's TS half (`tsEnvDump`) = the pob-ts dump: every modDB/enemyDB bucket as
//!       a MULTISET of `formatSourceMod|source` (L10/D13 -- `pairs(allocNodes)` order), conditions/multipliers key for key
//!       (multipliers through `%.17g`), every other field exactly;
//!   (b) Rust <-> LuaJIT classified exactly as pob-ts classifies it (`selfEnv` in export-env.ts = `initEnv-oracle.test.ts`):
//!       item-miss / node-miss / cluster / granted-passive / alloc-count / tree-jewel by the SOURCE of the mod, the gem
//!       multipliers (`Socketed*`/`Empty*Socket*`) excluded by name; per-build counts = R3.1, unclassified 0.
//! (a) and (b) run in `ParseMode::CacheOnly` (pob-ts). R5.4 adds (c): (b) in `ParseMode::Full` (the desktop: ModCache, else
//! the real ModParser) -- `item-miss`/`node-miss`/`granted-passive` 0, the rest (cluster, alloc-count, tree-jewel: R5.5)
//! pinned as measured.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{BuildModel, active_spec, load_build_xml};
use pob_calc::{Env, InitEnvOptions, ParseMode, init_env, item_options};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::{DataTable, Item};
use pob_lua::format_g;
use pob_mods::{StoreKind, StoreRef, Value};
use pob_tree::PassiveSpec;
use pob_tree::modtools::format_source_mod;
use serde_json::{Map, Value as Json, json};

/// R3.1 report / pob-ts pins: [item-miss, node-miss, cluster, granted-passive, alloc-count, tree-jewel].
const EXPECTED_CLASSES: [(&str, [usize; 6]); 6] = [
    ("Dual Savior", [0, 1, 0, 0, 0, 0]),
    ("Dual Wield Cospris CoC", [72, 12, 34, 1, 2, 12]),
    ("Dual Wield Cospris CoC - Lethal Pride", [72, 15, 34, 1, 2, 26]),
    ("Generals Perforate Zerker", [80, 28, 49, 4, 4, 0]),
    ("Mirage Archer Toxic Rain", [35, 13, 17, 3, 2, 0]),
    ("OccVortex", [36, 12, 0, 3, 0, 0]),
];
const CLASSES: [&str; 6] = ["item-miss", "node-miss", "cluster", "granted-passive", "alloc-count", "tree-jewel"];

fn fixture(name: &str, kind: &str) -> Json {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let p = dir.join(format!("{}.{kind}.json", name.replace(' ', "-")));
    let mut j: Json =
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json");
    j["env"].take()
}

/// The R3 run of one golden: items parsed one by one (ModCache misses recorded per item), the spec, a fresh `init_env`.
struct Run {
    env: Env,
    model: Rc<BuildModel>,
    miss_items: BTreeSet<u64>,
}

fn run(name: &str) -> Run {
    run_mode(name, ParseMode::CacheOnly)
}

/// `run` with every mod line read in `mode`; in `Full` a "miss" item is one with a line the parser could not answer.
fn run_mode(name: &str, mode: ParseMode) -> Run {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let model = Rc::new(load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("xml parses"));
    let mut items = IndexMap::new();
    let mut miss_items = BTreeSet::new();
    for im in &model.items {
        pob_parser::reset_parse_mod_stats();
        let item = Item::parse(&im.raw, &item_options(im, mode));
        if !pob_parser::parse_mod_stats().miss_lines.is_empty() {
            miss_items.insert(im.id as u64);
        }
        if item.base.is_some() {
            items.insert(im.id as u64, Rc::new(item));
        }
    }
    let spec = PassiveSpec::load_mode(active_spec(&model), mode).expect("spec");
    let env = init_env(
        model.clone(),
        InitEnvOptions {
            spec: Some(spec),
            items: Some(items),
            parse_mode: mode,
        },
    )
    .expect("init_env");
    Run { env, model, miss_items }
}

fn rawf(n: f64) -> String {
    format_g(n, 17)
}

/// `(function|table): 0x...` -> `<addr>` (export-env.ts `normAddr`).
fn norm_addr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find(": 0x") {
        let head = &rest[..p];
        let is_addr = head.ends_with("function") || head.ends_with("table");
        let tail = &rest[p + 4..];
        let hex = tail.bytes().take_while(u8::is_ascii_hexdigit).count();
        if is_addr && hex > 0 {
            out.push_str(head);
            out.push_str(": <addr>");
            rest = &tail[hex..];
        } else {
            out.push_str(&rest[..p + 4]);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

fn db_dump(store: &StoreRef) -> Json {
    let st = store.borrow();
    let StoreKind::Db(db) = &st.kind else { panic!("not a ModDB") };
    let mut mods = Map::new();
    let mut srcs = Map::new();
    for (name, bucket) in &db.mods {
        mods.insert(
            name.to_string(),
            json!(bucket.iter().map(|m| norm_addr(&format_source_mod(m))).collect::<Vec<_>>()),
        );
        srcs.insert(
            name.to_string(),
            json!(
                bucket
                    .iter()
                    .map(|m| m.source.as_deref().unwrap_or("").to_owned())
                    .collect::<Vec<_>>()
            ),
        );
    }
    let conditions: Map<String, Json> = st.conditions.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let multipliers: Map<String, Json> = st.multipliers.iter().map(|(k, v)| (k.to_string(), json!(rawf(*v)))).collect();
    json!({ "mods": mods, "srcs": srcs, "conditions": conditions, "multipliers": multipliers })
}

fn wd_dump(d: &DataTable) -> Json {
    let mut m = Map::new();
    for (k, v) in &d.0 {
        let j = match v {
            Value::Num(x) => json!(rawf(*x)),
            Value::Str(x) => json!(x.to_string()),
            Value::Bool(b) => json!(b),
            Value::Nil => continue,
            other => json!(format!("{other:?}")),
        };
        m.insert(k.clone(), j);
    }
    Json::Object(m)
}

/// export-env.ts `tsEnvDump`.
fn env_dump(r: &Run) -> Json {
    let env = &r.env;
    let p = env.player.borrow();
    let item_slots: Map<String, Json> = p.item_list.iter().map(|(k, it)| (k.clone(), json!(it.name))).collect();
    let mut alloc: Vec<u32> = env.alloc_nodes.keys().copied().collect();
    alloc.sort_unstable();
    let mut ks: Vec<&String> = env.keystones_added.keys().collect();
    ks.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut gp: Vec<u32> = env.granted_passives.keys().copied().collect();
    gp.sort_unstable();
    let initial_mods = match &env.initial_node_mod_db.borrow().kind {
        StoreKind::List(l) => l.mods.len(),
        StoreKind::Db(_) => unreachable!(),
    };
    json!({
        "modDB": db_dump(&env.mod_db),
        "enemyDB": db_dump(&env.enemy_db),
        "enemyLevel": env.enemy_level,
        "itemSlots": item_slots,
        "alloc": alloc,
        "weapon1": wd_dump(&p.weapon_data1),
        "weapon2": wd_dump(&p.weapon_data2),
        "mainSocketGroup": env.main_socket_group,
        "orderedSlots": env.ordered_slots.iter().map(|s| s.slot_name.clone()).collect::<Vec<_>>(),
        "keystonesAdded": ks,
        "grantedPassives": gp,
        "grantedSkills": env.granted_skills.len(),
        "reqItems": env.requirements_table_items.len(),
        "reqGems": env.requirements_table_gems.len(),
        "initialNodeMods": initial_mods,
        "classId": env.class_id,
        "level": r.model.level,
        "notableCount": env.spec.allocated_notable_count,
        "keystoneCount": env.spec.allocated_keystone_count,
    })
}

/// JSON numbers compared as f64 (pob-ts writes `84`, Rust `84.0`).
fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => x.as_f64() == y.as_f64(),
        (Json::Array(x), Json::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Json::Object(x), Json::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

fn bucket_pairs(db: &Json, name: &str) -> BTreeMap<(String, String), usize> {
    let mods = db["mods"][name].as_array().map(Vec::as_slice).unwrap_or_default();
    let srcs = db["srcs"][name].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut m = BTreeMap::new();
    for (i, f) in mods.iter().enumerate() {
        let src = srcs.get(i).and_then(Json::as_str).unwrap_or("").to_owned();
        *m.entry((f.as_str().expect("str").to_owned(), src)).or_insert(0) += 1;
    }
    m
}

/// (a) bucket multisets + conditions/multipliers key for key. Returns (differences, buckets whose ORDER differs).
fn diff_db_exact(label: &str, ours: &Json, theirs: &Json, out: &mut Vec<String>) -> usize {
    let names = |d: &Json| -> BTreeSet<String> { d["mods"].as_object().expect("mods").keys().cloned().collect() };
    let (a, b) = (names(ours), names(theirs));
    let mut order_diffs = 0;
    for n in a.union(&b) {
        let (x, y) = (bucket_pairs(ours, n), bucket_pairs(theirs, n));
        if x != y {
            out.push(format!("{label}.mods[{n}]: rust {x:?} != pob-ts {y:?}"));
        } else if ours["mods"][n] != theirs["mods"][n] {
            order_diffs += 1;
        }
    }
    for k in ["conditions", "multipliers"] {
        if ours[k] != theirs[k] {
            let (x, y) = (ours[k].as_object().unwrap(), theirs[k].as_object().unwrap());
            for key in x.keys().chain(y.keys()).collect::<BTreeSet<_>>() {
                if x.get(key) != y.get(key) {
                    out.push(format!("{label}.{k}[{key}]: rust {:?} != pob-ts {:?}", x.get(key), y.get(key)));
                }
            }
        }
    }
    order_diffs
}

#[test]
fn env_equals_pob_ts_for_six_goldens() {
    let mut report = Vec::new();
    for g in GOLDEN_BUILDS {
        let r = run(g.name);
        let ours = env_dump(&r);
        let theirs = fixture(g.name, "pobts");
        let mut diffs = Vec::new();
        let mut order = 0;
        for db in ["modDB", "enemyDB"] {
            order += diff_db_exact(db, &ours[db], &theirs[db], &mut diffs);
        }
        let ours_o = ours.as_object().unwrap();
        let theirs_o = theirs.as_object().unwrap();
        assert_eq!(
            ours_o.keys().collect::<BTreeSet<_>>(),
            theirs_o.keys().collect::<BTreeSet<_>>(),
            "{}: dump fields",
            g.name
        );
        for (k, v) in ours_o {
            if k == "modDB" || k == "enemyDB" {
                continue;
            }
            if !same(v, &theirs_o[k]) {
                diffs.push(format!("{k}: rust {v} != pob-ts {}", theirs_o[k]));
            }
        }
        let count = |d: &Json| -> (usize, usize) {
            let m = d["mods"].as_object().unwrap();
            (m.len(), m.values().map(|b| b.as_array().unwrap().len()).sum())
        };
        report.push(format!(
            "{}: modDB {:?} enemyDB {:?} (pob-ts {:?}/{:?}), diffs {}, buckets in a different order {order}",
            g.name,
            count(&ours["modDB"]),
            count(&ours["enemyDB"]),
            count(&theirs["modDB"]),
            count(&theirs["enemyDB"]),
            diffs.len()
        ));
        assert!(
            diffs.is_empty(),
            "{}: {} differences from pob-ts:\n{}",
            g.name,
            diffs.len(),
            diffs.join("\n")
        );
    }
    println!("{}", report.join("\n"));
}

// --- (b) LuaJIT, classified ---------------------------------------------------------------------------------------

/// `Item:<id>:` / `Tree:<id>` / `Tree<id>` (the keystoneMod source has no colon).
fn source_ref(s: &str) -> Option<(&'static str, u64)> {
    if let Some(rest) = s.strip_prefix("Item:") {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(':') {
            return Some(("item", digits.parse().ok()?));
        }
    }
    if let Some(rest) = s.strip_prefix("Tree") {
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(("tree", rest.parse().ok()?));
        }
    }
    None
}

/// `/^SocketedGemsIn/`, `/^Socketed(Red|Green|Blue)GemsIn/`, `/^EmptySocketIn/`, `/^Empty(Red|Green|Blue|White)SocketsInAnySlot$/`.
fn is_gem(k: &str) -> bool {
    k.starts_with("SocketedGemsIn")
        || ["Red", "Green", "Blue"]
            .iter()
            .any(|c| k.starts_with(&format!("Socketed{c}GemsIn")))
        || k.starts_with("EmptySocketIn")
        || ["Red", "Green", "Blue", "White"]
            .iter()
            .any(|c| k == format!("Empty{c}SocketsInAnySlot"))
}

const GENERIC_ITEM_SOURCES: [&str; 3] = ["Quality", "Item Sockets", "Unlinked Item Sockets"];

struct Gaps<'a> {
    r: &'a Run,
    oracle_only_alloc: BTreeSet<u64>,
    granted: BTreeSet<u64>,
    unequipped: BTreeSet<u64>,
    has_tree_jewel: bool,
}

impl Gaps<'_> {
    fn miss_node(&self, id: u64) -> bool {
        let spec = &self.r.env.spec;
        let id = id as u32;
        if let Some(n) = spec.node(id) {
            return n.unknown() || n.mods().iter().flatten().any(|m| m.extra.is_some());
        }
        spec.tree
            .node(id)
            .is_some_and(|n| n.stats.unknown || n.stats.mods.iter().flatten().any(|m| m.extra.is_some()))
    }

    fn classify(&self, name: &str, source: &str, oracle_only: bool) -> &'static str {
        match source_ref(source) {
            Some(("item", id)) => {
                if self.r.miss_items.contains(&id) {
                    return "item-miss";
                }
                if oracle_only && self.unequipped.contains(&id) {
                    return "cluster";
                }
            }
            Some(("tree", id)) => {
                if oracle_only && self.oracle_only_alloc.contains(&id) {
                    return if self.granted.contains(&id) { "granted-passive" } else { "cluster" };
                }
                if self.miss_node(id) {
                    return "node-miss";
                }
                if self.has_tree_jewel {
                    return "tree-jewel";
                }
            }
            _ => {}
        }
        if name.starts_with("Multiplier:Allocated") && !self.oracle_only_alloc.is_empty() {
            return "alloc-count";
        }
        if oracle_only && GENERIC_ITEM_SOURCES.contains(&source) && !self.unequipped.is_empty() {
            return "cluster";
        }
        "unexplained"
    }
}

fn classify_db(label: &str, ours: &Json, lj: &Json, gaps: &Gaps, cls: &mut BTreeMap<&'static str, Vec<String>>) {
    let mut src: BTreeMap<(String, String), String> = BTreeMap::new();
    for d in [lj, ours] {
        for (n, b) in d["mods"].as_object().unwrap() {
            for (i, f) in b.as_array().unwrap().iter().enumerate() {
                let key = (n.clone(), f.as_str().unwrap().to_owned());
                let s = d["srcs"][n][i].as_str().unwrap_or("").to_owned();
                src.entry(key).or_insert(s);
            }
        }
    }
    let count = |d: &Json, n: &str| -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for f in d["mods"][n].as_array().map(Vec::as_slice).unwrap_or_default() {
            *m.entry(f.as_str().unwrap().to_owned()).or_insert(0) += 1;
        }
        m
    };
    let names: BTreeSet<String> = ours["mods"]
        .as_object()
        .unwrap()
        .keys()
        .chain(lj["mods"].as_object().unwrap().keys())
        .cloned()
        .collect();
    for n in &names {
        let (a, b) = (count(ours, n), count(lj, n));
        for s in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
            let (x, y) = (a.get(s).copied().unwrap_or(0), b.get(s).copied().unwrap_or(0));
            if x == y {
                continue;
            }
            let source = src.get(&(n.clone(), s.clone())).cloned().unwrap_or_default();
            let c = gaps.classify(n, &source, y > x);
            cls.entry(c)
                .or_default()
                .push(format!("{label}.mods[{n}] RSx{x} LJx{y} src={source} :: {s}"));
        }
    }
    let (am, bm) = (obj(&ours["multipliers"]), obj(&lj["multipliers"]));
    for k in am.keys().chain(bm.keys()).collect::<BTreeSet<_>>() {
        if is_gem(k) || am.get(k) == bm.get(k) {
            continue;
        }
        let c = gaps.classify(&format!("Multiplier:{k}"), "", bm.contains_key(k));
        cls.entry(c)
            .or_default()
            .push(format!("{label}.multipliers[{k}]: RS {:?} != LJ {:?}", am.get(k), bm.get(k)));
    }
    let (ac, bc) = (obj(&ours["conditions"]), obj(&lj["conditions"]));
    for k in ac.keys().chain(bc.keys()).collect::<BTreeSet<_>>() {
        if ac.get(k) != bc.get(k) {
            cls.entry("unexplained").or_default().push(format!("{label}.conditions[{k}]"));
        }
    }
}

#[test]
fn env_vs_luajit_classified_as_pob_ts() {
    env_vs_luajit(ParseMode::CacheOnly, &EXPECTED_CLASSES);
}

/// R5.4 measured, `ParseMode::Full`: [item-miss, node-miss, cluster, granted-passive, alloc-count, tree-jewel].
const EXPECTED_CLASSES_FULL: [(&str, [usize; 6]); 6] = [
    ("Dual Savior", [0, 0, 0, 0, 0, 0]),
    ("Dual Wield Cospris CoC", [0, 0, 66, 0, 2, 12]),
    ("Dual Wield Cospris CoC - Lethal Pride", [0, 0, 66, 0, 2, 29]),
    ("Generals Perforate Zerker", [0, 0, 84, 0, 4, 0]),
    ("Mirage Archer Toxic Rain", [0, 0, 21, 0, 2, 0]),
    ("OccVortex", [0, 0, 0, 0, 0, 0]),
];

/// R5.4 gate (c): the classification of (b) with the real parser behind every line.
#[test]
fn env_full_vs_luajit_classified() {
    env_vs_luajit(ParseMode::Full, &EXPECTED_CLASSES_FULL);
}

fn env_vs_luajit(mode: ParseMode, expected_classes: &[(&str, [usize; 6]); 6]) {
    let mut table = Vec::new();
    let mut failures = Vec::new();
    for &(name, expected) in expected_classes {
        let r = run_mode(name, mode);
        let ours = env_dump(&r);
        let lj = fixture(name, "luajit");
        let env = &r.env;
        let p = env.player.borrow();

        let lj_alloc: BTreeSet<u64> = lj["alloc"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
        let oracle_only_alloc: BTreeSet<u64> = lj_alloc
            .iter()
            .copied()
            .filter(|id| !env.alloc_nodes.contains_key(&(*id as u32)))
            .collect();
        let mut violations = Vec::new();
        let mut unequipped = BTreeSet::new();
        for slot in obj(&lj["itemSlots"]).keys() {
            if p.item_list.contains_key(slot) {
                continue;
            }
            let item = slot
                .strip_prefix("Jewel ")
                .and_then(|n| n.parse::<f64>().ok())
                .and_then(|n| env.spec.jewels.get(n).copied());
            match item {
                Some(id) => {
                    unequipped.insert(id as u64);
                }
                None => violations.push(format!("luajit equipped {slot}, rust has no item for it")),
            }
        }
        let granted: BTreeSet<u64> = lj["grantedPassives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        let has_tree_jewel = env.unimplemented.iter().any(|u| u.contains("radius jewel"))
            || env
                .items
                .values()
                .any(|i| i.base.and_then(|b| b.sub_type.as_deref()) == Some("Timeless"));
        let gaps = Gaps {
            r: &r,
            oracle_only_alloc: oracle_only_alloc.clone(),
            granted: granted.clone(),
            unequipped,
            has_tree_jewel,
        };
        let mut cls: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        classify_db("modDB", &ours["modDB"], &lj["modDB"], &gaps, &mut cls);
        classify_db("enemyDB", &ours["enemyDB"], &lj["enemyDB"], &gaps, &mut cls);

        // The scalar checks of selfEnv.
        let mut chk = |ok: bool, m: String| {
            if !ok {
                violations.push(m);
            }
        };
        chk(same(&ours["enemyLevel"], &lj["enemyLevel"]), "enemyLevel".into());
        chk(same(&ours["mainSocketGroup"], &lj["mainSocketGroup"]), "mainSocketGroup".into());
        chk(ours["orderedSlots"] == lj["orderedSlots"], "orderedSlots".into());
        chk(same(&ours["classId"], &lj["classId"]), "classId".into());
        chk(
            ours["grantedSkills"].as_u64() <= lj["grantedSkills"].as_u64()
                && ours["initialNodeMods"].as_u64() <= lj["initialNodeMods"].as_u64(),
            "grantedSkills/initialNodeModDB over-count".into(),
        );
        for id in env.alloc_nodes.keys() {
            chk(lj_alloc.contains(&u64::from(*id)), format!("allocNodes: rust has {id}, luajit not"));
        }
        for id in &oracle_only_alloc {
            let id32 = *id as u32;
            let explained = env.spec.node(id32).is_none()
                || env
                    .spec
                    .tree
                    .node(id32)
                    .is_some_and(|n| n.expansion_jewel.as_ref().is_some_and(|e| e.parent.is_some()))
                || granted.contains(id);
            chk(explained, format!("luajit-only node {id} unexplained"));
        }
        for (slot, it) in &p.item_list {
            chk(
                lj["itemSlots"][slot].as_str() == Some(it.name.as_str()),
                format!("itemList[{slot}]"),
            );
        }
        chk(
            !oracle_only_alloc.is_empty() || ours["keystonesAdded"] == lj["keystonesAdded"],
            "keystonesAdded".into(),
        );
        for (slot, ours_wd, lj_wd) in [
            ("Weapon 1", &ours["weapon1"], &lj["weapon1"]),
            ("Weapon 2", &ours["weapon2"], &lj["weapon2"]),
        ] {
            if p.item_list
                .get(slot)
                .and_then(|i| i.id)
                .is_some_and(|id| r.miss_items.contains(&(id as u64)))
            {
                continue;
            }
            let empty = Map::new();
            let (a, b) = (ours_wd.as_object().unwrap(), lj_wd.as_object().unwrap_or(&empty));
            for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                chk(a.get(k) == b.get(k), format!("{slot} weaponData[{k}]"));
            }
        }

        let counts: Vec<usize> = CLASSES.iter().map(|c| cls.get(c).map_or(0, Vec::len)).collect();
        let unexplained = cls.get("unexplained").cloned().unwrap_or_default();
        let lj_count = |d: &Json| -> (usize, usize) {
            let m = d["mods"].as_object().unwrap();
            (m.len(), m.values().map(|b| b.as_array().unwrap().len()).sum())
        };
        table.push(format!(
            "{name}: classes {counts:?} (R3.1 {expected:?}), unexplained {}, violations {}, alloc rust/lj {}/{}, itemSlots {}/{}, LuaJIT modDB {:?} enemyDB {:?}",
            unexplained.len(),
            violations.len(),
            env.alloc_nodes.len(),
            lj_alloc.len(),
            p.item_list.len(),
            obj(&lj["itemSlots"]).len(),
            lj_count(&lj["modDB"]),
            lj_count(&lj["enemyDB"]),
        ));
        if !unexplained.is_empty() {
            failures.push(format!(
                "{name}: {} UNEXPLAINED:\n{}",
                unexplained.len(),
                unexplained[..unexplained.len().min(40)].join("\n")
            ));
        }
        if !violations.is_empty() {
            failures.push(format!("{name}: {violations:?}"));
        }
        if counts != expected {
            failures.push(format!("{name}: class counts {counts:?} != {expected:?}"));
        }
    }
    println!("{mode:?}\n{}", table.join("\n"));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A Lua table dumped as JSON: an EMPTY table comes out of the LuaJIT probe as `[]`.
fn obj(j: &Json) -> Map<String, Json> {
    match j {
        Json::Object(m) => m.clone(),
        Json::Array(a) if a.is_empty() => Map::new(),
        other => panic!("expected an object, got {other}"),
    }
}

/// The comparator of (a) sees a changed multiplier, a dropped mod and a changed source.
#[test]
fn comparator_is_not_vacuous() {
    let r = run("Dual Savior");
    let ours = env_dump(&r);
    let mut bad = ours.clone();
    bad["modDB"]["multipliers"]["Level"] = json!("2");
    bad["modDB"]["mods"]["Str"].as_array_mut().unwrap().pop();
    bad["modDB"]["srcs"]["Str"].as_array_mut().unwrap().pop();
    bad["enemyDB"]["srcs"]["Accuracy"][0] = json!("Elsewhere");
    let mut diffs = Vec::new();
    diff_db_exact("modDB", &ours["modDB"], &bad["modDB"], &mut diffs);
    diff_db_exact("enemyDB", &ours["enemyDB"], &bad["enemyDB"], &mut diffs);
    assert_eq!(diffs.len(), 3, "{diffs:?}");
}
