//! The item dump in the shape of the TS half of pob-ts `ITEM_DUMP` (`test/items/oracle.test.ts:303-362`, the same as
//! `tools/lua-corpus/export-env.ts` `tsItems`), plus the comparison helpers of that test (`norm`/`diff`, :366-400) and its
//! classification against LuaJIT (`selfItems`, export-env.ts:622-660). Shared by the fixture test and the perf example.

#![allow(dead_code)]

use std::collections::BTreeSet;

use pob_build::{ItemModel, load_build_xml};
use pob_golden::registry::GoldenBuild;
use pob_items::{DataTable, INFLUENCE_INFO, Item, ItemOptions, LINE_FLAGS, ModLine, SlotMod, format_source_mod};
use pob_mods::{Mod, Value};
pub use serde_json::Value as Json;
use serde_json::{Map, json};

/// test/items/oracle.test.ts:114-119.
pub const FIND_SUBSTRING_PROBES: [(&str, &str); 15] = [
    ("life", "body armour"),
    ("life", "amulet"),
    ("life", "belt"),
    ("life", "ring"),
    ("life", "helmet"),
    ("explicit life", "body armour"),
    ("evasion", "ring"),
    ("defence", "ring"),
    ("critical strike", "ring"),
    ("attack", "weapon 1"),
    ("%a+ damage", "ring"),
    ("^adds", "ring"),
    ("damage$", "ring"),
    ("-1", "ring"),
    ("%d+%% increased", "ring"),
];

const LINE_LISTS: [&str; 7] = [
    "enchantModLines",
    "scourgeModLines",
    "classRequirementModLines",
    "implicitModLines",
    "explicitModLines",
    "crucibleModLines",
    "buffModLines",
];

/// `fixtures/<build with spaces as '-'>.<side>.json`.
pub fn fixture(name: &str, side: &str) -> Json {
    let slug = name.replace(' ', "-");
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../pob-calc/tests/fixtures/{slug}.{side}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

pub fn options_of(im: &ItemModel, mode: pob_parser::ParseMode) -> ItemOptions {
    ItemOptions {
        parse_mode: mode,
        id: Some(im.id),
        variant: im.variant,
        variant_alt: [im.variant_alt, im.variant_alt2, im.variant_alt3, im.variant_alt4, im.variant_alt5],
        mod_ranges: im.mod_ranges.iter().map(|r| (r.id, r.range)).collect(),
    }
}

/// pob-ts `rawf` -- JS `toPrecision(17)` trimmed (the `%.17g` channel of O2), integers below 1e17 as digits.
pub fn rawf(n: f64) -> String {
    if n.is_nan() {
        return "nan".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if n.fract() == 0.0 && n.abs() < 1e17 {
        if n == 0.0 && n.is_sign_negative() {
            return "-0".into();
        }
        return format!("{}", n as i64);
    }
    let s = format!("{:.16e}", n);
    let (mant, exp) = s.split_once('e').expect("e");
    let x: i32 = exp.parse().expect("exp");
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let sign = if neg { "-" } else { "" };
    if !(-7..17).contains(&x) {
        let m = format!("{}.{}", &digits[..1], &digits[1..]);
        let m = m.trim_end_matches('0').trim_end_matches('.');
        return format!("{sign}{m}e{}{:02}", if x < 0 { '-' } else { '+' }, x.unsigned_abs());
    }
    let s = if x >= 0 {
        let i = x as usize + 1;
        format!("{}.{}", &digits[..i], &digits[i..])
    } else {
        format!("0.{}{}", "0".repeat((-x - 1) as usize), digits)
    };
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{sign}{s}")
}

/// JS `String(x)` for a dumped value.
fn js_string(v: &Value) -> String {
    match v {
        Value::Nil => "undefined".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 1e21 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Value::Str(s) => s.to_string(),
        Value::Table(t) if t.hash.is_empty() => t.array.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Table(_) | Value::Mod(_) => "[object Object]".into(),
        Value::Func(_) => pob_mods::FUNCTION_TOSTRING.into(),
    }
}

/// pob-ts `N(v)`.
fn n(v: &Value) -> Option<Json> {
    match v {
        Value::Nil => None,
        Value::Num(x) => Some(Json::String(rawf(*x))),
        Value::Bool(b) => Some(Json::Bool(*b)),
        other => Some(Json::String(js_string(other))),
    }
}

fn nf(v: Option<f64>) -> Option<Json> {
    v.map(|x| Json::String(rawf(x)))
}

fn put(o: &mut Map<String, Json>, k: &str, v: Option<Json>) {
    if let Some(v) = v {
        o.insert(k.to_owned(), v);
    }
}

fn fmt_mods(list: &[Mod]) -> Json {
    Json::Array(list.iter().map(|m| Json::String(format!("{}|nil", format_source_mod(m)))).collect())
}

fn fmt_slot(list: &[SlotMod]) -> Json {
    Json::Array(
        list.iter()
            .map(|m| Json::String(format!("{}|{}", format_source_mod(&m.m), m.source_slot.as_deref().unwrap_or("nil"))))
            .collect(),
    )
}

/// pob-ts `tblTs` for a record of values.
fn tbl_value_entries(t: &DataTable) -> Map<String, Json> {
    let mut out = Map::new();
    for (k, v) in &t.0 {
        match v {
            Value::Table(tt) => {
                let mut inner = Map::new();
                for (i, x) in tt.array.iter().enumerate() {
                    put(&mut inner, &(i + 1).to_string(), n(x));
                }
                for (k2, x) in &tt.hash {
                    put(&mut inner, k2, n(x));
                }
                out.insert(k.clone(), Json::Object(inner));
            }
            other => put(&mut out, k, n(other)),
        }
    }
    out
}

fn lines(list: &[ModLine]) -> Json {
    Json::Array(
        list.iter()
            .map(|ml| {
                let mut o = Map::new();
                o.insert("line".into(), json!(ml.line));
                put(&mut o, "mods", ml.mod_list.as_deref().map(fmt_mods));
                put(&mut o, "extra", ml.extra.clone().map(Json::String));
                put(&mut o, "range", nf(ml.range));
                put(&mut o, "valueScalar", nf(ml.value_scalar));
                put(&mut o, "corruptedRange", nf(ml.corrupted_range));
                o.insert("modTags".into(), json!(ml.mod_tags.join(",")));
                put(&mut o, "modGroup", ml.mod_group.clone().map(Json::String));
                put(&mut o, "showSlider", ml.show_slider.map(Json::Bool));
                let flags: Vec<&str> = LINE_FLAGS.iter().copied().filter(|f| ml.flag(f)).collect();
                o.insert("flags".into(), json!(flags.join(",")));
                if let Some(vl) = &ml.variant_list
                    && !vl.is_empty()
                {
                    let v: Vec<String> = vl.iter().map(|k| js_string(&Value::Num(k.0))).collect();
                    o.insert("variantList".into(), json!(v.join(",")));
                }
                Json::Object(o)
            })
            .collect(),
    )
}

/// One item in the TS dump shape.
pub fn dump_item(item: &Item) -> Json {
    let mut o = Map::new();
    let s = |v: &Option<String>| v.clone().map(Json::String);
    o.insert("name".into(), json!(item.name));
    put(&mut o, "title", s(&item.title));
    put(&mut o, "baseName", s(&item.base_name));
    put(&mut o, "type", s(&item.ty));
    o.insert("rarity".into(), json!(item.rarity));
    o.insert("namePrefix".into(), json!(item.name_prefix));
    o.insert("nameSuffix".into(), json!(item.name_suffix));
    put(&mut o, "quality", nf(item.quality));
    put(&mut o, "itemLevel", nf(item.item_level));
    for (k, b) in [
        ("corrupted", item.corrupted),
        ("split", item.split),
        ("mirrored", item.mirrored),
        ("fractured", item.fractured),
        ("synthesised", item.synthesised),
        ("crafted", item.crafted),
        ("advancedCopy", item.advanced_copy),
        ("isUnique", item.is_unique),
        ("vestigial", item.vestigial),
        ("foulborn", item.foulborn),
        ("hasModTags", item.has_mod_tags),
        ("veiled", item.veiled),
        ("usesVariantGroups", item.uses_variant_groups),
    ] {
        o.insert(k.into(), json!(b));
    }
    let infl: Vec<&str> = INFLUENCE_INFO.iter().map(|(k, _)| *k).filter(|k| item.influence(k)).collect();
    o.insert("influences".into(), json!(infl.join(",")));
    put(&mut o, "uniqueID", s(&item.unique_id));
    put(&mut o, "league", s(&item.league));
    put(&mut o, "classRestriction", s(&item.class_restriction));
    put(&mut o, "limit", nf(item.limit));
    put(&mut o, "jewelRadiusIndex", nf(item.jewel_radius_index));
    put(&mut o, "jewelRadiusLabel", s(&item.jewel_radius_label));
    put(&mut o, "catalyst", nf(item.catalyst));
    put(&mut o, "catalystQuality", nf(item.catalyst_quality));
    put(&mut o, "talismanTier", nf(item.talisman_tier));
    put(&mut o, "abyssalSocketCount", nf(Some(item.abyssal_socket_count)));
    put(&mut o, "selectableSocketCount", nf(item.selectable_socket_count));
    put(&mut o, "defaultSocketColor", s(&item.default_socket_color));
    put(&mut o, "socketedJewelEffectModifier", nf(item.socketed_jewel_effect_modifier));
    put(&mut o, "craftedQuality", nf(item.crafted_quality));
    put(&mut o, "corruptImplicitCount", nf(item.corrupt_implicit_count));
    put(&mut o, "modSource", s(&item.mod_source));
    put(&mut o, "clusterJewelSkill", s(&item.cluster_jewel_skill));
    put(&mut o, "clusterJewelNodeCount", nf(item.cluster_jewel_node_count));
    put(&mut o, "variant", nf(item.variant));
    put(&mut o, "variantAlt", nf(item.variant_alt[0]));
    put(&mut o, "selectedVersion", nf(item.selected_version));
    let r = &item.requirements;
    let mut req = Map::new();
    for (k, v) in [
        ("level", r.level),
        ("str", r.str),
        ("dex", r.dex),
        ("int", r.int),
        ("strMod", r.str_mod),
        ("dexMod", r.dex_mod),
        ("intMod", r.int_mod),
    ] {
        put(&mut req, k, nf(v));
    }
    o.insert("requirements".into(), Json::Object(req));
    let socks: Vec<String> = item
        .sockets
        .iter()
        .map(|s| format!("{}{}", s.color, js_string(&Value::Num(s.group))))
        .collect();
    o.insert("sockets".into(), json!(socks.join(" ")));
    put(
        &mut o,
        "armourData",
        item.armour_data.as_ref().map(|t| Json::Object(tbl_value_entries(t))),
    );
    if let Some(wd) = &item.weapon_data {
        let mut w = Map::new();
        for (k, t) in wd {
            w.insert(js_string(&Value::Num(k.0)), Json::Object(tbl_value_entries(t)));
        }
        o.insert("weaponData".into(), Json::Object(w));
    }
    put(
        &mut o,
        "flaskData",
        item.flask_data.as_ref().map(|t| Json::Object(tbl_value_entries(t))),
    );
    put(
        &mut o,
        "tinctureData",
        item.tincture_data.as_ref().map(|t| Json::Object(tbl_value_entries(t))),
    );
    put(
        &mut o,
        "jewelData",
        item.jewel_data.as_ref().map(|t| Json::Object(tbl_value_entries(t))),
    );
    let gs: Vec<String> = item
        .granted_skills
        .iter()
        .map(|g| {
            let f = |v: &Value| if v.is_nil() { "nil".to_owned() } else { js_string(v) };
            [
                f(&g.skill_id),
                f(&g.level),
                f(&g.triggered),
                g.source.clone(),
                f(&g.no_supports),
                f(&g.trigger_chance),
            ]
            .join("|")
        })
        .collect();
    o.insert("grantedSkills".into(), json!(gs.join("; ")));
    put(&mut o, "modList", item.mod_list.as_deref().map(fmt_slot));
    if let Some(slots) = &item.slot_mod_list {
        let mut m = Map::new();
        for (i, l) in slots.iter().enumerate() {
            m.insert((i + 1).to_string(), fmt_slot(l));
        }
        o.insert("slotModList".into(), Json::Object(m));
    }
    put(&mut o, "baseModList", item.base_mod_list.as_deref().map(fmt_mods));
    put(&mut o, "buffModList", item.buff_mod_list.as_deref().map(fmt_mods));
    for (name, list) in item.mod_line_lists() {
        o.insert(name.into(), lines(list));
    }
    put(&mut o, "rangeLineCount", nf(Some(item.range_line_count as f64)));
    let fms: String = FIND_SUBSTRING_PROBES
        .iter()
        .map(|(s, slot)| if item.find_modifier_substring_impl(s, slot) { '1' } else { '0' })
        .collect();
    o.insert("findModifierSubstring".into(), json!(fms));
    Json::Object(o)
}

pub struct GoldenRun {
    pub dump: Json,
    /// Per item id: the ModCache miss lines of its parse (pob-ts `missesByItem`).
    pub misses: Map<String, Json>,
    pub items: Vec<Item>,
    /// Ids of the items that recorded `Item.lua:1543 mutatedLines` as unimplemented.
    pub mutated: BTreeSet<String>,
}

/// pob-ts `tsItems`: parse every `<Item>` of the build (counting ModCache misses per item), keep those with a base --
/// `ParseMode::CacheOnly`, as pob-ts.
pub fn run_golden(g: &GoldenBuild) -> GoldenRun {
    run_golden_mode(g, pob_parser::ParseMode::CacheOnly)
}

/// `run_golden` in `mode`; in `Full` the per-item "misses" are the lines the parser could not answer either (R5.4).
pub fn run_golden_mode(g: &GoldenBuild, mode: pob_parser::ParseMode) -> GoldenRun {
    let xml = std::fs::read_to_string(g.xml_path()).unwrap_or_else(|e| panic!("{}: {e}", g.name));
    let model = load_build_xml(&xml).unwrap_or_else(|e| panic!("{}: {e}", g.name));
    let mut out = Map::new();
    let mut misses = Map::new();
    let mut items = Vec::new();
    for im in &model.items {
        pob_parser::reset_parse_mod_stats();
        let item = Item::parse(&im.raw, &options_of(im, mode));
        let key = js_string(&Value::Num(im.id));
        misses.insert(key.clone(), json!(pob_parser::parse_mod_stats().miss_lines));
        if item.base.is_none() {
            continue;
        }
        out.insert(key, dump_item(&item));
        items.push(item);
    }
    let mutated = items
        .iter()
        .filter(|i| i.unimplemented.iter().any(|u| u.contains(":1543 mutatedLines")))
        .map(|i| js_string(&Value::Num(i.id.unwrap_or(-1.0))))
        .collect();
    GoldenRun {
        dump: Json::Object(out),
        misses,
        items,
        mutated,
    }
}

// --- comparison ---------------------------------------------------------------------------------------------------

/// Exact equality (numbers bit for bit, same keys); differences as paths.
pub fn diff_exact(path: &str, want: &Json, got: &Json, out: &mut Vec<String>) {
    match (want, got) {
        (Json::Object(a), Json::Object(b)) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for k in keys {
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => diff_exact(&format!("{path}.{k}"), x, y, out),
                    (x, y) => out.push(format!("{path}.{k}: pob-ts {x:?}, rust {y:?}")),
                }
            }
        }
        (Json::Array(a), Json::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff_exact(&format!("{path}[{i}]"), x, y, out);
            }
        }
        (Json::Number(a), Json::Number(b)) => {
            if a.as_f64().map(f64::to_bits) != b.as_f64().map(f64::to_bits) {
                out.push(format!("{path}: pob-ts {a}, rust {b}"));
            }
        }
        _ => {
            if want != got {
                out.push(format!("{path}: pob-ts {want}, rust {got}"));
            }
        }
    }
}

/// oracle.test.ts:366 `norm`: dkjson writes an empty table as `[]` and drops nil; `""` is absent.
fn norm(v: &Json) -> Option<Json> {
    match v {
        Json::Null => None,
        Json::Array(a) => {
            if a.is_empty() {
                Some(json!("[]"))
            } else {
                Some(Json::Array(a.iter().map(|x| norm(x).unwrap_or(Json::Null)).collect()))
            }
        }
        Json::Object(o) => {
            let mut out = Map::new();
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            for k in keys {
                if let Some(n) = norm(&o[k.as_str()]) {
                    out.insert(k.clone(), n);
                }
            }
            if out.is_empty() {
                Some(json!("[]"))
            } else {
                Some(Json::Object(out))
            }
        }
        Json::String(s) if s.is_empty() => None,
        other => Some(other.clone()),
    }
}

/// oracle.test.ts:389 `diff` over normalised values.
fn diff_norm(path: &str, a: Option<&Json>, b: Option<&Json>, out: &mut Vec<String>) {
    let na = a.and_then(norm);
    let nb = b.and_then(norm);
    match (&na, &nb) {
        (Some(Json::Object(x)), Some(Json::Object(y))) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                diff_norm(&format!("{path}.{k}"), x.get(k), y.get(k), out);
            }
        }
        _ => {
            if na != nb {
                out.push(path.to_owned());
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct LuajitReport {
    pub items: usize,
    pub lines: usize,
    pub clean: usize,
    pub with_miss: usize,
    pub miss_lines: usize,
    /// Items with a `jewelData.funcList` difference (LuaJIT-only radius-jewel functions, P5); on a clean item it is the
    /// only difference allowed.
    pub tree_jewel: usize,
    /// R5.4 (visible once `item-miss` no longer hides it): items whose `rangeLineCount` differs AND that carry the
    /// explicit `Item.lua:1543 mutatedLines` unimplemented marker (foulborn transformation, P5 -- `modId`/`newModId` lines
    /// count as range lines upstream).
    pub mutated_lines: usize,
    /// R5.4: items whose only other difference is a NESTED table in `jewelData` (`conqueredBy.conqueror`): the LuaJIT probe
    /// prints `tostring(t)` = `table: <addr>`, the pob-ts-shaped dump `String(t)` = `[object Object]` -- both "a table".
    pub nested_table: usize,
    pub violations: Vec<String>,
}

const MISS_IMMUNE: [&str; 36] = [
    "name",
    "title",
    "baseName",
    "type",
    "rarity",
    "namePrefix",
    "nameSuffix",
    "itemLevel",
    "corrupted",
    "split",
    "mirrored",
    "fractured",
    "synthesised",
    "crafted",
    "advancedCopy",
    "isUnique",
    "vestigial",
    "foulborn",
    "veiled",
    "influences",
    "uniqueID",
    "league",
    "classRestriction",
    "limit",
    "jewelRadiusIndex",
    "jewelRadiusLabel",
    "catalyst",
    "catalystQuality",
    "talismanTier",
    "defaultSocketColor",
    "variant",
    "variantAlt",
    "selectedVersion",
    "usesVariantGroups",
    "modSource",
    "findModifierSubstring",
];
const MISS_IMMUNE_LINE: [&str; 5] = ["modTags", "modGroup", "flags", "variantList", "corruptedRange"];

fn line_texts(l: Option<&Json>) -> String {
    l.and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .map(|x| x["line"].as_str().unwrap_or("").to_owned())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// export-env.ts `selfItems` (= oracle.test.ts:449-510) with the `tree-jewel` class split out of the clean items.
pub fn items_vs_luajit(lj: &Json, rust: &Json, misses: &Map<String, Json>) -> LuajitReport {
    items_vs_luajit_with(lj, rust, misses, &BTreeSet::new())
}

/// `items_vs_luajit` knowing which items carry the `Item.lua:1543 mutatedLines` marker (R5.4 classes `mutated-lines`,
/// `nested-table`; both only apply to an exact path with the exact values below).
pub fn items_vs_luajit_with(lj: &Json, rust: &Json, misses: &Map<String, Json>, mutated: &BTreeSet<String>) -> LuajitReport {
    let mut r = LuajitReport::default();
    let empty = Map::new();
    let lj = lj.as_object().unwrap_or(&empty);
    let rust = rust.as_object().unwrap_or(&empty);
    let mut lk: Vec<&String> = lj.keys().collect();
    let mut rk: Vec<&String> = rust.keys().collect();
    lk.sort();
    rk.sort();
    if lk != rk {
        r.violations.push(format!("item ids differ: luajit {lk:?}, rust {rk:?}"));
    }
    let mut miss_set: BTreeSet<String> = BTreeSet::new();
    let mut ids: Vec<&String> = lj.keys().collect();
    ids.sort_by(|a, b| a.parse::<f64>().unwrap_or(0.0).total_cmp(&b.parse::<f64>().unwrap_or(0.0)));
    let null = Json::Null;
    for id in ids {
        let l = &lj[id.as_str()];
        let t = rust.get(id.as_str()).unwrap_or(&null);
        r.items += 1;
        for list in LINE_LISTS {
            r.lines += l.get(list).and_then(Json::as_array).map_or(0, Vec::len);
        }
        let mut differences = Vec::new();
        diff_norm(id, Some(l), Some(t), &mut differences);
        for list in LINE_LISTS {
            if line_texts(l.get(list)) != line_texts(t.get(list)) {
                r.violations.push(format!("{id}.{list} text"));
            }
            let a = l.get(list).and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default();
            let b = t.get(list).and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default();
            if a.len() == b.len() {
                for (i, (x, y)) in a.iter().zip(b).enumerate() {
                    for k in MISS_IMMUNE_LINE {
                        diff_norm(&format!("{id}.{list}.{i}.{k}"), x.get(k), y.get(k), &mut r.violations);
                    }
                }
            }
        }
        for k in MISS_IMMUNE {
            diff_norm(&format!("{id}.{k}"), l.get(k), t.get(k), &mut r.violations);
        }
        for k in ["level", "str", "dex", "int"] {
            if t.get("requirements").and_then(|x| x.get(k)) != l.get("requirements").and_then(|x| x.get(k)) {
                r.violations.push(format!("{id}.requirements.{k}"));
            }
        }
        let m: Vec<String> = misses
            .get(id.as_str())
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let mut rest: Vec<&String> = differences.iter().filter(|d| !d.contains(".jewelData.funcList")).collect();
        if rest.len() < differences.len() {
            r.tree_jewel += 1;
        }
        let range_path = format!("{id}.rangeLineCount");
        if mutated.contains(id.as_str()) && rest.iter().any(|d| **d == range_path) {
            rest.retain(|d| **d != range_path);
            r.mutated_lines += 1;
        }
        let nested = format!("{id}.jewelData.conqueredBy.conqueror");
        let lj_nested = l.pointer("/jewelData/conqueredBy/conqueror").and_then(Json::as_str);
        let rs_nested = t.pointer("/jewelData/conqueredBy/conqueror").and_then(Json::as_str);
        if rest.iter().any(|d| **d == nested) && lj_nested == Some("table: <addr>") && rs_nested == Some("[object Object]") {
            rest.retain(|d| **d != nested);
            r.nested_table += 1;
        }
        if m.is_empty() {
            r.clean += 1;
            if !rest.is_empty() {
                r.violations.push(format!(
                    "{id} {}: no ModCache miss yet {} diffs, first {}",
                    l["name"],
                    rest.len(),
                    rest[0]
                ));
            }
        } else {
            r.with_miss += 1;
            miss_set.extend(m);
        }
    }
    r.miss_lines = miss_set.len();
    r
}
