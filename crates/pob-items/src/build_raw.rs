//! Classes/Item.lua `BuildRaw` (:1772-2047) and `BuildAndParseRaw` (:2049-2052): the item text rebuilt from the parsed
//! state (what `ItemsTab:Save` :1349 writes into `<Item>`), plus the affix tables the crafted-item slots of `ParseRaw`
//! (:1474-1520) resolve against (`self.affixes`, Item.lua:1031-1038; `data.itemMods`, Data.lua:649-691;
//! `data.rareLikeUniques`, Data.lua:1338-1440).
//!
//! R13: upstream walks `pairs(self.baseLines)` (:1951/:1981) -- with two or more written base lines their order is LuaJIT's
//! string-hash order, which differs between processes. Here it is the insertion (text) order.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use indexmap::IndexMap;
use pob_data::item_bases::ItemBase;
use pob_data::wire::OrderedMap;

use crate::item::{AffixRange, INFLUENCE_INFO, Item, ModLine, NumSet};
use crate::lua::{gsub, matches, round, tostring};

const ITEM: &str = "Classes/Item.lua";

/// Item.lua:15-28 `catalystList`.
const CATALYST_LIST: [&str; 12] = [
    "Abrasive",
    "Accelerating",
    "Dextral",
    "Fertile",
    "Imbued",
    "Intrinsic",
    "Noxious",
    "Prismatic",
    "Sinistral",
    "Tempering",
    "Turbulent",
    "Unstable",
];

// --- affix tables ---------------------------------------------------------------------------------------------------

/// An affix table (`data.itemMods.<Category>` or `rareLikeUnique.affixes`): mod id -> `mod.affix`, in the data file's
/// order (the converter writes keys sorted, D13).
#[derive(Debug, Default)]
pub struct ItemAffixes {
    name: &'static str,
    by_id: IndexMap<String, String>,
    /// The data of the mods behind `by_id`.
    pub(crate) values: IndexMap<String, &'static serde_json::Value>,
    pub(crate) full: OnceLock<IndexMap<String, crate::craft::AffixMod>>,
}

impl ItemAffixes {
    /// `self.affixes[modId]` is not nil.
    pub fn contains(&self, mod_id: &str) -> bool {
        self.by_id.contains_key(mod_id)
    }

    /// Item.lua:1505-1510 `for modId, mod in pairs(self.affixes) do if list[i].modId == mod.affix then ... break`.
    /// R13: with several mods of that affix name the winner is the `pairs` order; here the file order, and the ambiguity
    /// is recorded on `unimplemented`.
    pub fn by_affix_name(&self, affix: &str, unimplemented: &mut Vec<String>) -> Option<String> {
        let mut hits = self.by_id.iter().filter(|(_, a)| a.as_str() == affix).map(|(id, _)| id);
        let first = hits.next()?.clone();
        let more = hits.count();
        if more > 0 {
            unimplemented.push(format!(
                "{ITEM}:1506 R13: affix name '{affix}' matches {} mods of {} (pairs order); took '{first}'",
                more + 1,
                self.name
            ));
        }
        Some(first)
    }
}

/// `data.itemMods` keys (Data.lua:649-668 + `Item` :686); WatchersEye is a converter extra, not an upstream key.
const ITEM_MOD_CATEGORIES: [&str; 16] = [
    "Explicit",
    "ItemExclusive",
    "Corrupted",
    "Delve",
    "Synthesis",
    "Scourge",
    "Eldritch",
    "Flask",
    "Tincture",
    "Graft",
    "Jewel",
    "JewelAbyss",
    "JewelCluster",
    "JewelCharm",
    "Foulborn",
    "Mercenary",
];
/// Data.lua:687 -- the categories merged into `data.itemMods.Item`, in order (later ones overwrite).
const ITEM_MERGE_ORDER: [&str; 8] = [
    "Explicit",
    "ItemExclusive",
    "Corrupted",
    "Delve",
    "Synthesis",
    "Scourge",
    "Eldritch",
    "Mercenary",
];

fn affix_of(v: &serde_json::Value) -> String {
    v.get("affix").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned()
}

fn read_mod_file(rel: &str) -> OrderedMap<serde_json::Value> {
    pob_data::read_json(rel).unwrap_or_else(|e| panic!("{e}"))
}

/// One parsed data file, cached (a crafted item resolves against it on every parse).
pub(crate) fn mod_file(rel: &'static str) -> &'static OrderedMap<serde_json::Value> {
    static FILES: OnceLock<Mutex<HashMap<&'static str, &'static OrderedMap<serde_json::Value>>>> = OnceLock::new();
    let mut m = FILES.get_or_init(Default::default).lock().expect("mod file cache");
    m.entry(rel).or_insert_with(|| Box::leak(Box::new(read_mod_file(rel))))
}

fn category_file(cat: &str) -> &'static str {
    match cat {
        "Explicit" => "item-mods/Explicit.json",
        "ItemExclusive" => "item-mods/ItemExclusive.json",
        "Corrupted" => "item-mods/Corrupted.json",
        "Delve" => "item-mods/Delve.json",
        "Synthesis" => "item-mods/Synthesis.json",
        "Scourge" => "item-mods/Scourge.json",
        "Eldritch" => "item-mods/Eldritch.json",
        "Flask" => "item-mods/Flask.json",
        "Tincture" => "item-mods/Tincture.json",
        "Graft" => "item-mods/Graft.json",
        "Jewel" => "item-mods/Jewel.json",
        "JewelAbyss" => "item-mods/JewelAbyss.json",
        "JewelCluster" => "item-mods/JewelCluster.json",
        "JewelCharm" => "item-mods/JewelCharm.json",
        "Foulborn" => "item-mods/Foulborn.json",
        "Mercenary" => "item-mods/Mercenary.json",
        other => panic!("unknown data.itemMods category {other}"),
    }
}

/// `data.itemMods[cat]` (`Item` = the merge of Data.lua:686-691; `Vestigial` = ItemExclusive `^Divergent`, :669-673).
fn item_mods(cat: &'static str) -> &'static ItemAffixes {
    static CATS: OnceLock<Mutex<HashMap<&'static str, &'static ItemAffixes>>> = OnceLock::new();
    let mut m = CATS.get_or_init(Default::default).lock().expect("item mods cache");
    if let Some(a) = m.get(cat) {
        return a;
    }
    let mut by_id = IndexMap::new();
    let mut values: IndexMap<String, &'static serde_json::Value> = IndexMap::new();
    match cat {
        "Item" => {
            for c in ITEM_MERGE_ORDER {
                for (k, v) in mod_file(category_file(c)).iter() {
                    by_id.insert(k.clone(), affix_of(v));
                    values.insert(k.clone(), v);
                }
            }
        }
        "Vestigial" => {
            for (k, v) in mod_file(category_file("ItemExclusive")).iter() {
                if k.starts_with("Divergent") {
                    by_id.insert(k.clone(), affix_of(v));
                    values.insert(k.clone(), v);
                }
            }
        }
        c => {
            for (k, v) in mod_file(category_file(c)).iter() {
                by_id.insert(k.clone(), affix_of(v));
                values.insert(k.clone(), v);
            }
        }
    }
    let a: &'static ItemAffixes = Box::leak(Box::new(ItemAffixes {
        name: cat,
        by_id,
        values,
        full: OnceLock::new(),
    }));
    m.insert(cat, a);
    a
}

/// Item.lua:1031-1033 `(self.base.subType and data.itemMods[type..subType]) or data.itemMods[type] or data.itemMods.Item`.
pub fn item_affixes_for_base(base: &ItemBase) -> &'static ItemAffixes {
    let key = |k: &str| -> Option<&'static str> { ITEM_MOD_CATEGORIES.iter().copied().chain(["Item", "Vestigial"]).find(|c| *c == k) };
    if let Some(st) = &base.sub_type
        && let Some(c) = key(&format!("{}{st}", base.ty))
    {
        return item_mods(c);
    }
    item_mods(key(&base.ty).unwrap_or("Item"))
}

/// One `data.rareLikeUniques[title]` (Data.lua:1338-1440): the affix table and the slot limits.
#[derive(Debug)]
pub struct RareLikeUnique {
    pub affixes: ItemAffixes,
    pub prefix_limit: f64,
    pub suffix_limit: f64,
    pub ignore_mod_type: bool,
    /// `supportsCustomModifiers`: the ids of the sources it allows.
    pub supports_custom_modifiers: Option<Vec<String>>,
    /// `validBases`: the `tags` of each base.
    pub valid_bases: Option<Vec<OrderedMap<bool>>>,
}

/// `data.itemMods.ItemExclusive` as full mods.
pub fn item_exclusive_mods() -> &'static IndexMap<String, crate::craft::AffixMod> {
    item_mods("ItemExclusive").mods()
}

/// `data.rareLikeUniques[title:lower()]`.
pub fn rare_like_unique(title_lower: &str) -> Option<&'static RareLikeUnique> {
    static S: OnceLock<IndexMap<String, RareLikeUnique>> = OnceLock::new();
    S.get_or_init(|| {
        let j: OrderedMap<serde_json::Value> = pob_data::read_json("rare-like-uniques.json").unwrap_or_else(|e| panic!("{e}"));
        let mut out = IndexMap::new();
        for (title, v) in j.iter() {
            let mut by_id = IndexMap::new();
            let mut values: IndexMap<String, &'static serde_json::Value> = IndexMap::new();
            for a in v.get("affixes").and_then(serde_json::Value::as_array).into_iter().flatten() {
                let id = a.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                let src = a.get("source").and_then(serde_json::Value::as_str).unwrap_or_default();
                let table: &'static OrderedMap<serde_json::Value> = match src {
                    "veiledMods" => mod_file("veiled-mods.json"),
                    s => match s.strip_prefix("itemMods.") {
                        Some(c) => mod_file(category_file(c)),
                        None => panic!("rare-like-uniques.json: unknown affix source {s}"),
                    },
                };
                let name = table.get(&id).map(affix_of).unwrap_or_default();
                if let Some(x) = table.get(&id) {
                    values.insert(id.clone(), x);
                }
                by_id.insert(id, name);
            }
            let num = |k: &str| {
                v.get(k)
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_else(|| panic!("rare-like-uniques.json: {title}.{k}"))
            };
            let name: &'static str = Box::leak(format!("rareLikeUniques[{title}]").into_boxed_str());
            out.insert(
                title.clone(),
                RareLikeUnique {
                    affixes: ItemAffixes {
                        name,
                        by_id,
                        values,
                        full: OnceLock::new(),
                    },
                    prefix_limit: num("prefixLimit"),
                    suffix_limit: num("suffixLimit"),
                    ignore_mod_type: v.get("ignoreModType").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    valid_bases: v.get("validBases").and_then(|x| x.get("bases")).and_then(serde_json::Value::as_array).map(|l| {
                        l.iter()
                            .filter_map(|b| b.get("base").and_then(|x| x.get("tags")))
                            .filter_map(|tags| serde_json::from_value(tags.clone()).ok())
                            .collect()
                    }),
                    supports_custom_modifiers: v
                        .get("supportsCustomModifiers")
                        .and_then(serde_json::Value::as_object)
                        .map(|o| o.keys().cloned().collect()),
                },
            );
        }
        out
    })
    .get(title_lower)
}

// --- BuildRaw -------------------------------------------------------------------------------------------------------

/// Item.lua:1803 `makeIdSpec(idList)`: the ids in key order, `,`-joined.
fn make_id_spec(ids: &NumSet) -> String {
    ids.iter().map(|k| tostring(k.0)).collect::<Vec<_>>().join(",")
}

/// `affix.range` written back (Item.lua:1810/1814).
fn affix_range(r: &AffixRange) -> String {
    match r {
        AffixRange::List(xs) => xs.iter().map(|x| tostring(*x)).collect::<Vec<_>>().join(","),
        AffixRange::Num(x) => tostring(round(*x, Some(3.0))),
    }
}

/// The parts of Item.lua:1842 `writeModLine(modLine)` that a base line (`self.baseLines[name]`, a line with only its
/// variant lists) and a mod line share.
struct LineSpec<'a> {
    line: &'a str,
    ml: Option<&'a ModLine>,
    variant_list: Option<&'a NumSet>,
    version_list: Option<&'a NumSet>,
    variant_group_list: Option<&'a NumSet>,
}

/// Item.lua:1842-1928 `writeModLine`.
fn write_mod_line(raw_lines: &mut Vec<String>, spec: LineSpec<'_>) {
    let mut line = spec.line.to_owned();
    let prepend_to_all_lines = |line: &mut String, prefix: &str| {
        *line = format!("{prefix}{}", gsub(line, "\n", &format!("\n{prefix}"), None));
    };
    let empty: Vec<String> = Vec::new();
    let mod_tags = spec.ml.map_or(&empty, |m| &m.mod_tags);
    if let Some(ml) = spec.ml {
        let flag = |k: &str| ml.flag(k);
        if let Some(range) = ml.range
            && matches(&line, "%(%-?[%d%.]+%-%-?[%d%.]+%)")
        {
            line = format!("{{range:{}}}{line}", tostring(round(range, Some(6.0))));
        }
        if let Some(cr) = ml.corrupted_range {
            line = format!("{{corruptedRange:{}}}{line}", tostring(round(cr, Some(2.0))));
        }
        for k in ["disabled", "crafted", "enchant", "custom", "scourge", "crucible", "mutated"] {
            if flag(k) {
                line = format!("{{{k}}}{line}");
            }
        }
        if let Some(group) = &ml.mod_group {
            let mod_group = format!("{{modGroup:{group}}}");
            prepend_to_all_lines(&mut line, &mod_group);
        }
        for k in [
            "fractured",
            "prefix",
            "suffix",
            "exarch",
            "eater",
            "synthesis",
            "unscalable",
            "vestigial",
        ] {
            if flag(k) {
                line = format!("{{{k}}}{line}");
            }
        }
    }
    let has_new_selection = spec.version_list.is_some() || spec.variant_group_list.is_some();
    if has_new_selection && !mod_tags.is_empty() {
        line = format!("{{tags:{}}}{line}", mod_tags.join(","));
    }
    if let Some(g) = spec.variant_group_list {
        prepend_to_all_lines(&mut line, &format!("{{group:{}}}", make_id_spec(g)));
    }
    if let Some(v) = spec.variant_list {
        prepend_to_all_lines(&mut line, &format!("{{variant:{}}}", make_id_spec(v)));
    }
    if let Some(v) = spec.version_list {
        prepend_to_all_lines(&mut line, &format!("{{version:{}}}", make_id_spec(v)));
    }
    if !has_new_selection && !mod_tags.is_empty() {
        line = format!("{{tags:{}}}{line}", mod_tags.join(","));
    }
    raw_lines.push(line);
}

fn mod_line_spec(ml: &ModLine) -> LineSpec<'_> {
    LineSpec {
        line: &ml.line,
        ml: Some(ml),
        variant_list: ml.variant_list.as_ref(),
        version_list: ml.version_list.as_ref(),
        variant_group_list: ml.variant_group_list.as_ref(),
    }
}

/// `string.format("%d", n)` on a LuaJIT number: truncated toward zero.
fn fmt_d(n: f64) -> String {
    format!("{}", n.trunc() as i64)
}

fn nil_concat(line: u32, field: &str) -> ! {
    panic!("{ITEM}:{line}: attempt to concatenate field '{field}' (a nil value)")
}

impl Item {
    /// Item.lua:1772-2047 `BuildRaw`.
    pub fn build_raw(&self) -> String {
        let mut raw: Vec<String> = Vec::new();
        raw.push(format!("Rarity: {}", self.rarity));
        let base_name = || self.base_name.as_deref().unwrap_or_else(|| nil_concat(1776, "baseName"));
        if let Some(title) = &self.title {
            raw.push(title.clone());
            raw.push(base_name().to_owned());
        } else {
            raw.push(format!("{}{}{}", self.name_prefix, base_name(), self.name_suffix));
        }
        if let Some(ad) = &self.armour_data {
            for ty in ["Armour", "Evasion", "EnergyShield", "Ward"] {
                if let Some(v) = ad.num(ty)
                    && v > 0.0
                {
                    let label = if ty == "EnergyShield" { "Energy Shield" } else { ty };
                    raw.push(format!("{label}: {}", tostring(v)));
                    let pkey = format!("{ty}BasePercentile");
                    if !ad.get(&pkey).is_nil() {
                        let p = ad.num(&pkey).unwrap_or_else(|| nil_concat(1786, &pkey));
                        raw.push(format!("{pkey}: {}", tostring(p)));
                    }
                }
            }
        }
        if let Some(i) = self.intangibility {
            raw.push(format!("Intangibility: {}%", fmt_d(i)));
        }
        if let Some(id) = &self.unique_id {
            raw.push(format!("Unique ID: {id}"));
        }
        if let Some(league) = &self.league {
            raw.push(format!("League: {league}"));
        }
        if self.unreleased == Some(true) {
            raw.push("Unreleased: true".into());
        }
        for (i, (_, display)) in INFLUENCE_INFO.iter().enumerate() {
            if self.influence[i] {
                raw.push(format!("{display} Item"));
            }
        }
        if self.crafted {
            raw.push("Crafted: true".into());
            for (label, list) in [("Prefix", &self.prefixes), ("Suffix", &self.suffixes)] {
                for affix in list {
                    let range = affix
                        .range
                        .as_ref()
                        .map_or(String::new(), |r| format!("{{range:{}}}", affix_range(r)));
                    let fractured = if affix.fractured { "{fractured}" } else { "" };
                    raw.push(format!("{label}: {fractured}{range}{}", affix.mod_id));
                }
            }
        }
        if let Some(c) = self.catalyst
            && c > 0.0
        {
            if c.fract() != 0.0 || c > CATALYST_LIST.len() as f64 {
                panic!("{ITEM}:1820: attempt to concatenate a nil value");
            }
            let name = CATALYST_LIST[c as usize - 1];
            raw.push(format!("Catalyst: {name}"));
        }
        if let Some(q) = self.catalyst_quality {
            raw.push(format!("CatalystQuality: {}", tostring(q)));
        }
        if self.cluster_jewel.is_some() {
            if let Some(s) = &self.cluster_jewel_skill {
                raw.push(format!("Cluster Jewel Skill: {s}"));
            }
            if let Some(n) = self.cluster_jewel_node_count {
                raw.push(format!("Cluster Jewel Node Count: {}", tostring(n)));
            }
        }
        if let Some(t) = self.talisman_tier {
            raw.push(format!("Talisman Tier: {}", tostring(t)));
        }
        if let Some(l) = self.item_level {
            raw.push(format!("Item Level: {}", tostring(l)));
        }
        if let Some(m) = self.memory_strands {
            raw.push(format!("Memory Strands: {}", tostring(m)));
        }
        let write_base_lines = |raw: &mut Vec<String>, with_variant: bool| {
            // R13: `pairs(self.baseLines)` upstream (module doc).
            for (name, bl) in &self.base_lines {
                let written = bl.version_list.is_some() || bl.variant_group_list.is_some() || (with_variant && bl.variant_list.is_some());
                if written {
                    write_mod_line(
                        raw,
                        LineSpec {
                            line: name,
                            ml: None,
                            variant_list: bl.variant_list.as_ref(),
                            version_list: bl.version_list.as_ref(),
                            variant_group_list: bl.variant_group_list.as_ref(),
                        },
                    );
                }
            }
        };
        if let Some(versions) = &self.version_list {
            for v in versions {
                raw.push(format!("Version: {v}"));
            }
            if let Some(sv) = self.selected_version {
                raw.push(format!("Selected Version: {}", tostring(sv)));
            }
        }
        if let Some(variants) = &self.variant_list {
            for v in variants {
                raw.push(format!("Variant: {v}"));
            }
            if self.uses_variant_groups {
                for g in self.variant_groups.keys() {
                    if let Some(v) = self.variant_group_selections.get(g) {
                        raw.push(format!("Selected Variant Group: {}={}", tostring(g.0), tostring(*v)));
                    }
                }
            } else {
                let v = self.variant.unwrap_or_else(|| nil_concat(1946, "variant"));
                raw.push(format!("Selected Variant: {}", tostring(v)));
            }
            write_base_lines(&mut raw, true);
            const ALT: [(&str, &str, u32); 5] = [
                ("Has Alt Variant", "Selected Alt Variant", 1958),
                ("Has Alt Variant Two", "Selected Alt Variant Two", 1962),
                ("Has Alt Variant Three", "Selected Alt Variant Three", 1966),
                ("Has Alt Variant Four", "Selected Alt Variant Four", 1970),
                ("Has Alt Variant Five", "Selected Alt Variant Five", 1974),
            ];
            for (i, (has, sel, line)) in ALT.iter().enumerate() {
                if !self.uses_variant_groups && self.has_alt_variant[i] {
                    raw.push(format!("{has}: true"));
                    let v = self.variant_alt[i].unwrap_or_else(|| nil_concat(*line, "variantAlt"));
                    raw.push(format!("{sel}: {}", tostring(v)));
                }
            }
            if self.allow_duplicate_variants {
                raw.push("Allow Duplicate Variants: true".into());
            }
        } else {
            write_base_lines(&mut raw, false);
        }
        if let Some(q) = self.quality {
            raw.push(format!("Quality: {}", tostring(q)));
        }
        if !self.sockets.is_empty() {
            let mut line = String::from("Sockets: ");
            for (i, socket) in self.sockets.iter().enumerate() {
                line.push_str(&socket.color);
                if let Some(next) = self.sockets.get(i + 1) {
                    line.push_str(if socket.group == next.group { "-" } else { " " });
                }
            }
            raw.push(line);
        }
        if let Some(l) = self.requirements.level {
            raw.push(format!("LevelReq: {}", tostring(l)));
        }
        if let Some(r) = &self.jewel_radius_label {
            raw.push(format!("Radius: {r}"));
        }
        if let Some(l) = self.limit {
            raw.push(format!("Limited to: {}", tostring(l)));
        }
        if let Some(c) = &self.class_restriction {
            raw.push(format!("Requires Class {c}"));
        }
        let n_implicits = self.enchant_mod_lines.len() + self.implicit_mod_lines.len() + self.scourge_mod_lines.len();
        raw.push(format!("Implicits: {n_implicits}"));
        for list in [
            &self.enchant_mod_lines,
            &self.scourge_mod_lines,
            &self.class_requirement_mod_lines,
            &self.implicit_mod_lines,
            &self.explicit_mod_lines,
            &self.crucible_mod_lines,
        ] {
            for ml in list {
                write_mod_line(&mut raw, mod_line_spec(ml));
            }
        }
        if self.split {
            raw.push("Split".into());
        }
        if self.mirrored {
            raw.push("Mirrored".into());
        }
        if self.fractured {
            raw.push("Fractured Item".into());
        }
        if self.corrupted || self.scourge {
            raw.push("Corrupted".into());
        }
        if let Some(f) = &self.foil_type {
            raw.push(format!("Foil Unique ({f})"));
        }
        raw.join("\n")
    }

    /// Item.lua:2049-2052 `BuildAndParseRaw`.
    pub fn build_and_parse_raw(&mut self) {
        let raw = self.build_raw();
        self.parse_raw(&raw, None, false);
    }
}
