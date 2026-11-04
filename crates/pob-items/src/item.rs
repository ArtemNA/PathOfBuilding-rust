//! Classes/Item.lua -- `ParseRaw` (:425-1646), `BuildModList` (:2534-2704), `BuildModListForSlotNum` (:2222-2517),
//! `FindModifierSubstring` (:289-333) and the variant helpers, ported line for line in the PoB-XML scope of CALC-ARCH §5.
//! Executable spec: pob-ts `src/items/item.ts`; the truth is the Lua (patterns are upstream's, through `pob_lua`, L13).
//!
//! Skipped upstream branches are recorded on `Item::unimplemented` with their `file:line` (never silently dropped).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::OnceLock;

use indexmap::IndexMap;
use pob_data::item_bases::{ItemBase, item_bases};
use pob_mods::{Mod, ModStore, ModType, Str, Table, Tag, Value, create_mod};

use crate::apply_range::{apply_range, is_zero_value_line};
use crate::lua::{
    escape_ggg_string, find, find_plain, gmatch, gsub, gsub_count, gsub_fn, lower, match_, match1, matches, round, tonumber, tostring,
};
use pob_mods::format::set_source;

const ITEM: &str = "Classes/Item.lua";

/// Modules/Main.lua:113 -- the headless default (no user setting).
pub const DEFAULT_ITEM_AFFIX_QUALITY: f64 = 0.5;

/// Item.lua:13.
pub const DMG_TYPE_LIST: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
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
const CATALYST_DESCRIPTOR_LIST: [&str; 12] = [
    "Attack",
    "Speed",
    "Suffix",
    "Life and Mana",
    "Caster",
    "Attribute",
    "Physical and Chaos",
    "Resistance",
    "Prefix",
    "Defence",
    "Elemental",
    "Critical",
];
const CATALYST_TAGS: [&[&str]; 12] = [
    &["attack"],
    &["speed"],
    &["suffix"],
    &["life", "mana", "resource"],
    &["caster"],
    &["jewellery_attribute", "attribute"],
    &["physical_damage", "chaos_damage"],
    &["jewellery_resistance", "resistance"],
    &["prefix"],
    &["jewellery_defense", "defences", "armour", "evasion", "energyshield"],
    &["jewellery_elemental", "elemental_damage"],
    &["critical"],
];

/// ItemTools.lua:15-33 `itemLib.influenceInfo.all` (key, display), in order; the first 6 are `default`.
pub const INFLUENCE_INFO: [(&str, &str); 8] = [
    ("shaper", "Shaper"),
    ("elder", "Elder"),
    ("adjudicator", "Warlord"),
    ("basilisk", "Hunter"),
    ("crusader", "Crusader"),
    ("eyrie", "Redeemer"),
    ("cleansing", "Searing Exarch"),
    ("tangle", "Eater of Worlds"),
];

/// Item.lua:112-134 `lineFlags`.
pub const LINE_FLAGS: [&str; 17] = [
    "crafted",
    "crucible",
    "custom",
    "disabled",
    "eater",
    "enchant",
    "exarch",
    "fractured",
    "implicit",
    "scourge",
    "synthesis",
    "mutated",
    "unscalable",
    "prefix",
    "suffix",
    "unveiled",
    "vestigial",
];

fn line_flag_index(k: &str) -> Option<usize> {
    LINE_FLAGS.iter().position(|f| *f == k)
}

// --- numeric table keys -----------------------------------------------------------------------------------------

/// A Lua number used as a table key; ordered for the deterministic walks pob-ts uses (`pairsSortByKey`, D13).
#[derive(Clone, Copy, Debug)]
pub struct NumKey(pub f64);

impl PartialEq for NumKey {
    fn eq(&self, o: &Self) -> bool {
        self.0.total_cmp(&o.0) == Ordering::Equal
    }
}
impl Eq for NumKey {}
impl PartialOrd for NumKey {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for NumKey {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// A Lua set keyed by numbers (`variantList[id] = true`).
pub type NumSet = BTreeSet<NumKey>;

fn set_has(set: &NumSet, k: Option<f64>) -> bool {
    k.is_some_and(|k| set.contains(&NumKey(k)))
}

// --- the data model ---------------------------------------------------------------------------------------------

/// One mod line (`---@class ModLine`, Item.lua:412-423).
#[derive(Clone, Debug, Default)]
pub struct ModLine {
    pub line: String,
    pub mod_list: Option<Vec<Mod>>,
    pub extra: Option<String>,
    pub range: Option<f64>,
    pub value_scalar: Option<f64>,
    pub display_value_scalar: Option<f64>,
    pub corrupted_range: Option<f64>,
    pub variant_list: Option<NumSet>,
    pub version_list: Option<NumSet>,
    pub variant_group_list: Option<NumSet>,
    pub mod_tags: Vec<String>,
    pub mod_group: Option<String>,
    pub mod_id: Option<String>,
    pub new_mod_id: Option<String>,
    pub show_slider: Option<bool>,
    /// `modLine.order` (Item.lua:1374): the ItemExclusive stat order of a unique's explicit line, read by
    /// `sortCraftedModLines` (:72).
    pub order: Option<f64>,
    /// `modLine.rangeFromValue` (Item.lua:1188, 3dbca4463 #10374): the range came from a `value(min-max)` line, so a saved
    /// `<ModRange>` does not overwrite it (ItemsTab.lua:1307-1309).
    pub range_from_value: bool,
    /// `modLine.addedFromBase` (Item.lua:1643): an enchant line added from `base.enchant`, not in the saved text (ItemsTab.lua:1255).
    pub added_from_base: bool,
    /// `lineFlags`, indexed like `LINE_FLAGS`.
    pub flags: [bool; 17],
}

impl ModLine {
    pub fn flag(&self, k: &str) -> bool {
        line_flag_index(k).is_some_and(|i| self.flags[i])
    }
    fn set_flag(&mut self, k: &str) {
        if let Some(i) = line_flag_index(k) {
            self.flags[i] = true;
        }
    }
    /// `modLine[k] = true`.
    pub fn set_flag_pub(&mut self, k: &str) {
        self.set_flag(k);
    }
    /// `modLine[k] = v`.
    pub fn set_flag_to(&mut self, k: &str, v: bool) {
        if let Some(i) = line_flag_index(k) {
            self.flags[i] = v;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Socket {
    pub color: String,
    pub group: f64,
}

/// `item.requirements` -- every field a Lua number or nil.
#[derive(Clone, Debug, Default)]
pub struct Requirements {
    pub level: Option<f64>,
    pub str: Option<f64>,
    pub dex: Option<f64>,
    pub int: Option<f64>,
    pub str_mod: Option<f64>,
    pub dex_mod: Option<f64>,
    pub int_mod: Option<f64>,
}

/// One `item.grantedSkills` entry (Item.lua:2626-2633); the fields are whatever the ExtraSkill value holds.
#[derive(Clone, Debug)]
pub struct GrantedSkill {
    pub skill_id: Value,
    pub level: Value,
    pub no_supports: Value,
    pub source: String,
    pub triggered: Value,
    pub trigger_chance: Value,
}

/// A mod of a slot mod list with `mod.sourceSlot` (Item.lua:2242), which upstream writes onto the copied mod table.
#[derive(Clone, Debug)]
pub struct SlotMod {
    pub m: Mod,
    pub source_slot: Option<Str>,
}

/// A Lua record table of the `*Data` kind (`weaponData[1]`, `armourData`, `flaskData`, ...): string keys, any value;
/// assigning nil removes the key.
#[derive(Clone, Debug, Default)]
pub struct DataTable(pub IndexMap<String, Value>);

impl DataTable {
    pub fn get(&self, k: &str) -> Value {
        self.0.get(k).cloned().unwrap_or_default()
    }
    pub fn num(&self, k: &str) -> Option<f64> {
        self.0.get(k).and_then(Value::as_num)
    }
    pub fn set(&mut self, k: &str, v: Value) {
        if v.is_nil() {
            self.0.shift_remove(k);
        } else {
            self.0.insert(k.to_owned(), v);
        }
    }
    fn set_num(&mut self, k: &str, v: f64) {
        self.set(k, Value::Num(v));
    }
}

/// ItemsTab.lua:1216-1266: what `<Item>` carries besides its text.
#[derive(Clone, Debug, Default)]
pub struct ItemOptions {
    pub id: Option<f64>,
    pub variant: Option<f64>,
    /// `variantAlt`, `variantAlt2` .. `variantAlt5` (each present attribute also sets `hasAltVariantN`).
    pub variant_alt: [Option<f64>; 5],
    /// `<ModRange id range>` in document order.
    pub mod_ranges: Vec<(f64, f64)>,
    /// How mod lines are read (R5.4): `Full` (desktop, default) or `CacheOnly` (pob-ts).
    pub parse_mode: pob_parser::ParseMode,
}

#[derive(Clone, Debug, Default)]
pub struct Item {
    /// How this item's mod lines are read (`ItemOptions::parse_mode`).
    pub parse_mode: pob_parser::ParseMode,
    pub id: Option<f64>,
    pub raw: String,
    pub name: String,
    pub name_prefix: String,
    pub name_suffix: String,
    pub title: Option<String>,
    pub base_name: Option<String>,
    pub base: Option<&'static ItemBase>,
    pub ty: Option<String>,
    pub rarity: String,
    pub quality: Option<f64>,
    pub item_level: Option<f64>,
    pub unique_id: Option<String>,
    pub league: Option<String>,
    pub source: Option<String>,
    pub note: Option<String>,
    pub foil_type: Option<String>,
    pub talisman_tier: Option<f64>,
    pub memory_strands: Option<f64>,
    pub intangibility: Option<f64>,
    pub class_restriction: Option<String>,
    pub catalyst: Option<f64>,
    pub catalyst_quality: Option<f64>,
    pub limit: Option<f64>,
    pub jewel_radius_label: Option<String>,
    pub jewel_radius_index: Option<f64>,
    pub upgrade_paths: Option<Vec<String>>,
    pub unreleased: Option<bool>,
    pub is_unique: bool,
    pub hidden_specs: bool,

    pub split: bool,
    pub mirrored: bool,
    pub corrupted: bool,
    pub fractured: bool,
    pub synthesised: bool,
    pub veiled: bool,
    pub vestigial: bool,
    pub foulborn: bool,
    pub crafted: bool,
    pub scourge: bool,
    pub crucible: bool,
    pub implicit: bool,
    pub corruptible: bool,
    pub can_be_influenced: bool,
    pub implicits_cannot_be_changed: bool,
    pub can_be_anointed: bool,
    pub can_have_two_enchants: bool,
    pub can_have_three_enchants: bool,
    pub can_have_four_enchants: bool,
    pub has_elder_shaper_and_all_conqueror_influences: bool,
    pub can_have_eldritch_influence: bool,
    pub can_have_only_support_skills_crucible_tree: bool,
    pub can_have_shield_crucible_tree: bool,
    pub can_have_two_handed_sword_crucible_tree: bool,
    pub restrict_tag: bool,
    pub no_caster: bool,
    pub no_attack: bool,
    pub restrict_damage_type: bool,
    /// `self.only<Type>Damage`, in the order of [`crate::craft::DAMAGE_TYPES`].
    pub only_damage_type: [bool; 5],
    /// `self[influence.key] = true`, indexed like `INFLUENCE_INFO`.
    pub influence: [bool; 8],

    pub variant_list: Option<Vec<String>>,
    pub version_list: Option<Vec<String>>,
    pub variant: Option<f64>,
    /// `variantAlt`, `variantAlt2` .. `variantAlt5`.
    pub variant_alt: [Option<f64>; 5],
    pub has_alt_variant: [bool; 5],
    pub selected_version: Option<f64>,
    pub allow_duplicate_variants: bool,
    pub uses_variant_groups: bool,
    pub variant_groups: BTreeMap<NumKey, BTreeMap<NumKey, NumSet>>,
    pub variant_group_selections: BTreeMap<NumKey, f64>,

    pub class_requirement_mod_lines: Vec<ModLine>,
    pub buff_mod_lines: Vec<ModLine>,
    pub enchant_mod_lines: Vec<ModLine>,
    pub scourge_mod_lines: Vec<ModLine>,
    pub implicit_mod_lines: Vec<ModLine>,
    pub explicit_mod_lines: Vec<ModLine>,
    pub crucible_mod_lines: Vec<ModLine>,
    /// `#rangeLineList` -- how many lines got a slider (Item.lua:2577-2584); only the count is read (UI list otherwise).
    pub range_line_count: usize,
    pub advanced_copy: bool,
    mod_magnitude_mods: Vec<ModMagnitudeMod>,
    pub has_mod_tags: bool,
    pub prefixes_limit: Option<f64>,
    pub suffixes_limit: Option<f64>,

    pub requirements: Requirements,
    pub sockets: Vec<Socket>,
    pub sockets_colour_always_matches: bool,
    pub abyssal_socket_count: f64,
    pub selectable_socket_count: Option<f64>,
    pub default_socket_color: Option<String>,
    pub socketed_jewel_effect_modifier: Option<f64>,
    pub crafted_quality: Option<f64>,
    pub corrupt_implicit_count: Option<f64>,
    pub cluster_jewel: Option<&'static pob_data::cluster_jewels::ClusterJewelInfo>,
    pub cluster_jewel_skill: Option<String>,
    pub cluster_jewel_node_count: Option<f64>,
    pub mod_source: Option<String>,
    /// `baseModList` after `calcLocal` took the local mods out (the same ModList object upstream).
    pub base_mod_list: Option<Vec<Mod>>,
    pub mod_list: Option<Vec<SlotMod>>,
    /// `slotModList[1..2(3)]`, 0-based here.
    pub slot_mod_list: Option<Vec<Vec<SlotMod>>>,
    pub buff_mod_list: Option<Vec<Mod>>,
    /// `weaponData[slotNum]`.
    pub weapon_data: Option<BTreeMap<NumKey, DataTable>>,
    pub armour_data: Option<DataTable>,
    pub flask_data: Option<DataTable>,
    pub tincture_data: Option<DataTable>,
    pub jewel_data: Option<DataTable>,
    pub granted_skills: Vec<GrantedSkill>,

    pub check_section: bool,
    /// Every upstream branch this port knowingly skips on THIS item (CALC-ARCH §11).
    pub unimplemented: Vec<String>,

    /// `self.baseLines` (Item.lua:1005): base name -> the variant lists of its line. Upstream it is a hash walked with
    /// `pairs` by BuildRaw (R13: the order of 2+ written base lines is LuaJIT's hash order); here insertion order.
    pub base_lines: IndexMap<String, BaseLine>,
    /// `self.prefixes` / `self.suffixes` (Item.lua:512-513, 799-819, 1497-1519) -- the affix slots of a crafted item.
    pub prefixes: Vec<Affix>,
    pub suffixes: Vec<Affix>,
    /// `self.affixLimit` (Item.lua:1474).
    pub affix_limit: f64,
    /// `self.affixes` (Item.lua:1031-1038): the affix table of the first matched base -- NOT reset by ParseRaw.
    pub affixes: Option<&'static crate::build_raw::ItemAffixes>,
    /// `self.rareLikeUnique` (Item.lua:1036, reset by ParseRaw :520).
    pub rare_like_unique: Option<&'static crate::build_raw::RareLikeUnique>,
}

/// One `self.baseLines[baseName]` entry (Item.lua:1005-1010); `line` is the key.
#[derive(Clone, Debug, Default)]
pub struct BaseLine {
    pub variant_list: Option<NumSet>,
    pub version_list: Option<NumSet>,
    pub variant_group_list: Option<NumSet>,
}

/// `affix.range` (Item.lua:803-815): a number, or a list from `{range:a,b}`.
#[derive(Clone, Debug, PartialEq)]
pub enum AffixRange {
    Num(f64),
    List(Vec<f64>),
}

/// One `self.prefixes[i]` / `self.suffixes[i]` entry: `{ modId, range, fractured }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Affix {
    pub mod_id: String,
    pub range: Option<AffixRange>,
    pub fractured: bool,
}

#[derive(Clone, Debug)]
struct ModMagnitudeMod {
    tags: Vec<String>,
    any_tags: Option<[String; 2]>,
    quality: f64,
    multiplier: Option<f64>,
    mod_type: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    FindImplicit,
    Implicit,
    FindExplicit,
    Explicit,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListId {
    Enchant,
    Scourge,
    ClassRequirement,
    Implicit,
    Explicit,
    Crucible,
}

/// Item.lua:335 `specToNumber`.
fn spec_to_number(s: &str) -> Option<f64> {
    match1(s, "^([%+%-]?[%d%.]+)").and_then(|n| tonumber(&n))
}

/// Item.lua:31-62 `getCatalystScalar`.
fn get_catalyst_scalar(catalyst_id: Option<f64>, m: &ModLine, quality: Option<f64>) -> f64 {
    if m.flag("unscalable") {
        return 1.0;
    }
    let Some(tags) = catalyst_id
        .filter(|c| c.fract() == 0.0 && *c >= 1.0 && *c <= CATALYST_TAGS.len() as f64)
        .map(|c| CATALYST_TAGS[c as usize - 1])
    else {
        return 1.0;
    };
    if m.mod_tags.is_empty() {
        return 1.0;
    }
    let quality = quality.unwrap_or(20.0);
    let has = |t: &str| m.mod_tags.iter().any(|x| x == t) || ((t == "prefix" || t == "suffix") && m.flag(t));
    if tags.iter().any(|t| has(t)) {
        (100.0 + quality) / 100.0
    } else {
        1.0
    }
}

/// `data.foulbornMap` (Item.lua:1028): title -> `{ origModId = foulModId }`. File order stands for `pairs` order (D13: the
/// converter writes keys sorted); it only matters if two pairs of one item matched the same mod line.
fn foulborn_map() -> &'static pob_data::wire::OrderedMap<pob_data::wire::OrderedMap<String>> {
    static S: OnceLock<pob_data::wire::OrderedMap<pob_data::wire::OrderedMap<String>>> = OnceLock::new();
    S.get_or_init(|| pob_data::read_json("foulborn-map.json").unwrap_or_else(|e| panic!("{e}")))
}

/// Item.lua:64 `normaliseModLine` (first return value only, as every caller takes).
fn normalise_mod_line(line: &str) -> String {
    let s = gsub(line, "%d+%.?%d*", "#", None);
    let s = gsub(&s, "%(%-?#%-#%)", "#", None);
    gsub(
        &lower(&s),
        "
",
        " ",
        None,
    )
}

/// Item.lua:1361-1371 `uniqueModStatOrder` (`exact`, `normalised`): the smallest ItemExclusive `statOrder` per mod line.
fn unique_mod_stat_order() -> &'static (std::collections::HashMap<String, f64>, std::collections::HashMap<String, f64>) {
    static S: OnceLock<(std::collections::HashMap<String, f64>, std::collections::HashMap<String, f64>)> = OnceLock::new();
    S.get_or_init(|| {
        let (mut exact, mut normalised) = (std::collections::HashMap::new(), std::collections::HashMap::new());
        let file = crate::build_raw::mod_file("item-mods/ItemExclusive.json");
        for (id, m) in file.iter() {
            for (index, line) in m.lines.iter().enumerate() {
                // `m_min(x, nil)` is a Lua error upstream.
                let o = *m
                    .stat_order
                    .get(index)
                    .unwrap_or_else(|| panic!("{ITEM}:1367: ItemExclusive {id}: statOrder[{}] is nil", index + 1));
                let exact_line = gsub(
                    &lower(line),
                    "
",
                    " ",
                    None,
                );
                let e = exact.entry(exact_line).or_insert(f64::INFINITY);
                *e = pob_lua::number::min2(*e, o);
                let n = normalised.entry(normalise_mod_line(line)).or_insert(f64::INFINITY);
                *n = pob_lua::number::min2(*n, o);
            }
        }
        (exact, normalised)
    })
}

/// Item.lua:72-87 `sortCraftedModLines`: fractured, then the rest by `order` (nil last), then crafted/custom, each
/// group in source order (the comparator is total, so `table.sort` is deterministic here).
pub(crate) fn sort_crafted_mod_lines(mod_lines: &mut Vec<ModLine>) {
    let group = |m: &ModLine| -> u8 {
        if m.flag("crafted") || m.flag("custom") {
            3
        } else if m.flag("fractured") {
            1
        } else {
            2
        }
    };
    let mut indexed: Vec<(usize, ModLine)> = std::mem::take(mod_lines).into_iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| {
        let (ga, gb) = (group(a), group(b));
        if ga != gb {
            return ga.cmp(&gb);
        }
        if ga < 3 && a.order != b.order {
            let (oa, ob) = (a.order.unwrap_or(f64::INFINITY), b.order.unwrap_or(f64::INFINITY));
            // `(a.order or math.huge) < (b.order or math.huge)`; equal (nil vs huge) falls to the source order.
            if oa < ob {
                return Ordering::Less;
            }
            if ob < oa {
                return Ordering::Greater;
            }
        }
        ia.cmp(ib)
    });
    *mod_lines = indexed.into_iter().map(|(_, m)| m).collect();
}

/// `item_bases().names_sorted()`, computed once (the partial-match walk of Item.lua:972 runs per mod line of a MAGIC item).
fn base_names_sorted() -> &'static [&'static str] {
    static S: OnceLock<Vec<&'static str>> = OnceLock::new();
    S.get_or_init(|| item_bases().names_sorted())
}

fn mod_flag(name: &str) -> u32 {
    pob_data::globals::globals().mod_flag.get(name).copied().expect("ModFlag") as u32
}

fn keyword_flag(name: &str) -> u32 {
    pob_data::globals::globals().keyword_flag.get(name).copied().expect("KeywordFlag") as u32
}

fn tag(pairs: &[(&str, Value)]) -> Tag {
    let mut t = Table::default();
    for (k, v) in pairs {
        t.hash.insert(Rc::from(*k), v.clone());
    }
    Tag::new(t)
}

impl Item {
    /// ItemsTab.lua:1216-1266: `new("Item"):Item("")`, the XML attributes, `ParseRaw(text)`, `<ModRange>`, and
    /// `BuildModList` again when the base resolved (the caller keeps the item only then).
    pub fn parse(raw: &str, opts: &ItemOptions) -> Item {
        let mut item = Item {
            parse_mode: opts.parse_mode,
            ..Item::default()
        };
        // Item.lua:91 `Item("")`: "" is truthy, so ParseRaw runs on the empty string first.
        item.parse_raw("", None, false);
        item.id = opts.id;
        item.variant = opts.variant;
        for (i, v) in opts.variant_alt.iter().enumerate() {
            if v.is_some() {
                item.has_alt_variant[i] = true;
                item.variant_alt[i] = *v;
            }
        }
        item.parse_raw(raw, None, false);
        // ItemsTab.lua:1253-1256 `countSavedModLines`: enchants added from the base data were not in the saved text, so saved
        // ModRange ids skip the whole list (3dbca4463 #10374/#10376).
        let saved = |list: &[ModLine]| {
            if list.first().is_some_and(|m| m.added_from_base) {
                0
            } else {
                list.len()
            }
        };
        // :1283-1296 more ModRange entries than saved mod lines -> the ids no longer match their lines: keep the `{range:}` tags
        let ignore_mod_ranges = opts.mod_ranges.len()
            > saved(&item.enchant_mod_lines)
                + item.scourge_mod_lines.len()
                + item.implicit_mod_lines.len()
                + item.explicit_mod_lines.len()
                + item.crucible_mod_lines.len();
        for &(id, range) in &opts.mod_ranges {
            if ignore_mod_ranges {
                break;
            }
            let mut id = id;
            let lists: [&mut Vec<ModLine>; 6] = [
                &mut item.buff_mod_lines,
                &mut item.enchant_mod_lines,
                &mut item.scourge_mod_lines,
                &mut item.implicit_mod_lines,
                &mut item.explicit_mod_lines,
                &mut item.crucible_mod_lines,
            ];
            for list in lists {
                let saved_line_count = saved(list) as f64;
                if id <= saved_line_count {
                    // `list[id]` -- a non-positive or fractional id indexes nil (L18).
                    if id < 1.0 || id.fract() != 0.0 {
                        panic!("Classes/ItemsTab.lua:1307: attempt to index a nil value (field '?')");
                    }
                    // :1306-1309 versions that could not parse "value(min-max)" lines saved them as is with a default ModRange
                    let ml = &mut list[id as usize - 1];
                    if !ml.range_from_value {
                        ml.range = Some(range);
                    }
                    break;
                }
                id -= saved_line_count;
            }
        }
        if item.base.is_some() {
            item.build_mod_list();
        }
        item
    }

    /// `new("Item"):Item(raw, rarity, highQuality)` (Main.lua:166, :181).
    pub fn parse_with(raw: &str, rarity: Option<&str>, high_quality: bool) -> Item {
        let mut item = Item {
            parse_mode: pob_parser::ParseMode::Full,
            ..Item::default()
        };
        item.parse_raw(raw, rarity, high_quality);
        if item.base.is_some() {
            item.build_mod_list();
        }
        item
    }

    fn list(&self, id: ListId) -> &Vec<ModLine> {
        match id {
            ListId::Enchant => &self.enchant_mod_lines,
            ListId::Scourge => &self.scourge_mod_lines,
            ListId::ClassRequirement => &self.class_requirement_mod_lines,
            ListId::Implicit => &self.implicit_mod_lines,
            ListId::Explicit => &self.explicit_mod_lines,
            ListId::Crucible => &self.crucible_mod_lines,
        }
    }

    fn list_mut(&mut self, id: ListId) -> &mut Vec<ModLine> {
        match id {
            ListId::Enchant => &mut self.enchant_mod_lines,
            ListId::Scourge => &mut self.scourge_mod_lines,
            ListId::ClassRequirement => &mut self.class_requirement_mod_lines,
            ListId::Implicit => &mut self.implicit_mod_lines,
            ListId::Explicit => &mut self.explicit_mod_lines,
            ListId::Crucible => &mut self.crucible_mod_lines,
        }
    }

    /// `self[influence.key]`.
    pub fn influence(&self, key: &str) -> bool {
        INFLUENCE_INFO
            .iter()
            .position(|(k, _)| *k == key)
            .is_some_and(|i| self.influence[i])
    }

    // --- Item.lua:289-333 ---------------------------------------------------------------------------------------

    /// `FindModifierSubstring(substring, itemSlotName)`: `substring` is a Lua PATTERN (upstream passes it to `find`).
    pub fn find_modifier_substring_impl(&self, substring: &str, item_slot_name: &str) -> bool {
        let (substring, explicit) = gsub_count(substring, "explicit ", "");
        let mut mod_lines: Vec<&ModLine> = self.explicit_mod_lines.iter().collect();
        if explicit < 1 {
            mod_lines.extend(self.enchant_mod_lines.iter());
            mod_lines.extend(self.scourge_mod_lines.iter());
            mod_lines.extend(self.implicit_mod_lines.iter());
            mod_lines.extend(self.crucible_mod_lines.iter());
        }
        let rules = pob_data::rules::rules();
        let site = "Classes/Item.lua:309";
        for v in mod_lines {
            if v.flag("disabled") || !self.check_mod_line_variant(v) {
                continue;
            }
            let l = lower(&v.line);
            if find(&l, &substring, site) && !find(&l, &format!("{substring} modifier"), site) {
                let mut excluded = false;
                if let Some(list) = rules
                    .item_tag_special_exclusion_pattern
                    .get(&substring)
                    .and_then(|m| m.get(item_slot_name))
                {
                    for special in list {
                        if find(&l, &lower(special), site) {
                            excluded = true;
                            break;
                        }
                    }
                }
                if !excluded {
                    return true;
                }
            }
            if let Some(list) = rules.item_tag_special.get(&substring).and_then(|m| m.get(item_slot_name)) {
                for special in list {
                    if find(&l, &lower(special), site) {
                        return true;
                    }
                }
            }
        }
        false
    }

    // --- Item.lua:340-410 ---------------------------------------------------------------------------------------

    pub fn is_variant_group_option_eligible(&self, group_id: f64, variant_id: f64) -> bool {
        let Some(versions) = self.variant_groups.get(&NumKey(group_id)).and_then(|g| g.get(&NumKey(variant_id))) else {
            return false;
        };
        versions.contains(&NumKey(0.0)) || set_has(versions, self.selected_version)
    }

    pub fn get_variant_group_options(&self, group_id: f64, exclude_selected: bool) -> Vec<f64> {
        let mut options = Vec::new();
        if !self.variant_groups.contains_key(&NumKey(group_id)) {
            return options;
        }
        let mut used: Vec<f64> = Vec::new();
        if exclude_selected {
            for other in self.variant_groups.keys() {
                if other.0 != group_id
                    && let Some(&v) = self.variant_group_selections.get(other)
                    && self.is_variant_group_option_eligible(other.0, v)
                {
                    used.push(v);
                }
            }
        }
        let n = self.variant_list.as_ref().map_or(0, Vec::len);
        for variant_id in 1..=n {
            let v = variant_id as f64;
            if self.is_variant_group_option_eligible(group_id, v) && !used.contains(&v) {
                options.push(v);
            }
        }
        options
    }

    pub fn normalise_variant_selections(&mut self) {
        match &self.version_list {
            Some(vl) if !vl.is_empty() => {
                let n = vl.len() as f64;
                self.selected_version = Some(pob_lua::number::max2(
                    1.0,
                    pob_lua::number::min2(n, self.selected_version.unwrap_or(n)),
                ));
            }
            _ => self.selected_version = None,
        }
        let groups = &self.variant_groups;
        self.variant_group_selections.retain(|g, _| groups.contains_key(g));
        let mut used: Vec<f64> = Vec::new();
        let mut needs_selection: Vec<f64> = Vec::new();
        for g in self.variant_groups.keys().map(|k| k.0).collect::<Vec<_>>() {
            if self.get_variant_group_options(g, false).is_empty() {
                continue;
            }
            match self.variant_group_selections.get(&NumKey(g)) {
                Some(&sel) if self.is_variant_group_option_eligible(g, sel) && !used.contains(&sel) => used.push(sel),
                _ => needs_selection.push(g),
            }
        }
        for g in needs_selection {
            let selected = self.get_variant_group_options(g, false).into_iter().find(|v| !used.contains(v));
            match selected {
                Some(s) => {
                    self.variant_group_selections.insert(NumKey(g), s);
                    used.push(s);
                }
                None => {
                    self.variant_group_selections.remove(&NumKey(g));
                }
            }
        }
    }

    /// Item.lua:1553-1596 `checkMod(modId, newModId, mutated)`.
    fn check_mutated_mod(&mut self, mod_id: &str, new_mod_id: &str, mutated: bool) {
        let table = if mutated {
            pob_data::item_mods::foulborn()
        } else {
            pob_data::item_mods::item_exclusive()
        };
        // "mod not found while testing mutated mods" is a ConPrintf only.
        let Some(original_mod) = table.get(mod_id) else { return };
        let lines = &original_mod.lines;
        // Some modifier descriptions are represented by separate lines in PoB.
        let matching = self
            .find_matching_mod_lines(&[lines.join(" ")])
            .or_else(|| self.find_matching_mod_lines(lines));
        if let Some(matching) = matching {
            for i in matching {
                let ml = &mut self.explicit_mod_lines[i];
                ml.mod_id = Some(mod_id.to_owned());
                ml.new_mod_id = Some(new_mod_id.to_owned());
                if mutated {
                    ml.set_flag("mutated");
                }
            }
        }
    }

    /// Item.lua:1558-1578 `findMatchingLines`: indices into `explicit_mod_lines`, or None unless every line matched.
    fn find_matching_mod_lines(&self, lines: &[String]) -> Option<Vec<usize>> {
        let mut matching = Vec::new();
        for line in lines {
            let stat_line = normalise_mod_line(line);
            for (i, ml) in self.explicit_mod_lines.iter().enumerate() {
                if !matching.contains(&i) && normalise_mod_line(&ml.line) == stat_line && self.check_mod_line_variant(ml) {
                    matching.push(i);
                    break;
                }
            }
        }
        (lines.len() == matching.len()).then_some(matching)
    }

    // --- Item.lua:2120-2180 -------------------------------------------------------------------------------------

    pub fn check_mod_line_variant(&self, ml: &ModLine) -> bool {
        if self.uses_variant_groups {
            if let Some(vl) = &ml.version_list
                && !set_has(vl, self.selected_version)
            {
                return false;
            }
            if let Some(groups) = &ml.variant_group_list {
                let Some(variants) = &ml.variant_list else { return false };
                // `pairs(modLine.variantGroupList)` -- any order gives the same answer.
                return groups
                    .iter()
                    .any(|g| set_has(variants, self.variant_group_selections.get(g).copied()));
            }
            return ml.variant_list.is_none();
        }
        let Some(list) = &ml.variant_list else { return true };
        set_has(list, self.variant) || (0..5).any(|i| self.has_alt_variant[i] && set_has(list, self.variant_alt[i]))
    }

    pub fn get_mod_line_variant_count(&self, ml: &ModLine) -> usize {
        let Some(list) = ml.variant_list.as_ref().filter(|_| self.allow_duplicate_variants) else {
            return usize::from(self.check_mod_line_variant(ml));
        };
        let mut count = usize::from(set_has(list, self.variant));
        for i in 0..5 {
            if self.has_alt_variant[i] && set_has(list, self.variant_alt[i]) {
                count += 1;
            }
        }
        count
    }

    /// Item.lua:2164-2180 `GetPrimarySlot`.
    pub fn get_primary_slot(&self) -> String {
        let base = self.base.expect("GetPrimarySlot needs a base");
        let ty = self.ty.as_deref();
        if base.weapon.is_some() {
            "Weapon 1".into()
        } else if ty == Some("Quiver") || ty == Some("Shield") {
            "Weapon 2".into()
        } else if ty == Some("Ring") {
            "Ring 1".into()
        } else if ty == Some("Flask") || ty == Some("Tincture") {
            "Flask 1".into()
        } else if ty == Some("Graft") {
            "Graft 1".into()
        } else {
            self.ty.clone().expect("item.type")
        }
    }

    /// Item.lua:1683-1691 `NormaliseQuality`.
    pub fn normalise_quality(&mut self) {
        if let Some(base) = self.base
            && (base.armour.is_some() || base.weapon.is_some() || base.flask.is_some() || base.tincture.is_some())
        {
            match self.quality {
                None => self.quality = Some(0.0),
                Some(q) if self.unique_id.is_none() && !self.corrupted && !self.split && !self.mirrored && q < 20.0 => {
                    self.quality = Some(20.0);
                }
                _ => {}
            }
        }
    }

    // --- Item.lua:425-1646 `ParseRaw` -------------------------------------------------------------------------------

    pub fn parse_raw(&mut self, raw: &str, rarity: Option<&str>, high_quality: bool) {
        self.raw = raw.to_owned();
        self.name = "?".into();
        self.name_prefix = String::new();
        self.name_suffix = String::new();
        self.base = None;
        self.rarity = rarity.unwrap_or("UNIQUE").to_owned();
        self.quality = None;
        // Item.lua:432 `raw:gmatch("%s*([^\n]*%S)")` + escapeGGGString
        let raw_lines: Vec<String> = gmatch(raw, "%s*([^\n]*%S)").into_iter().map(|c| escape_ggg_string(&c[0])).collect();
        let at = |i: usize| -> Option<&str> { raw_lines.get(i.wrapping_sub(1)).map(String::as_str) };
        let mut mode_game = rarity.is_some();
        let mut l: usize = 1;
        let mut item_class: Option<String> = None;
        if let Some(first) = at(l) {
            if matches(first, "^Item Class:") {
                // `gsub("^Item Class: %s+", "%1")` -- no captures: `%1` is the whole match, a no-op replacement.
                item_class = Some(gsub(first, "^Item Class: %s+", "%1", None));
                l += 1;
            }
            let line = at(l).unwrap_or_else(|| panic!("{ITEM}:444: attempt to index a nil value (field '?')"));
            if let Some(r) = match1(line, "^Rarity: (%a+)") {
                mode_game = true;
                let up = pob_lua::string::upper(&r);
                if pob_data::globals::globals().color_codes.contains_key(&up) {
                    self.rarity = up;
                }
                if self.rarity == "UNIQUE" || self.rarity == "RELIC" {
                    for line in &raw_lines {
                        if find(line, "Foil Unique", ITEM) {
                            self.rarity = "RELIC".into();
                            self.foil_type = Some(match1(line, "%((.-)%)").unwrap_or_else(|| "Rainbow".into()));
                            break;
                        }
                    }
                }
                l += 1;
            }
        }
        if at(l).is_some() {
            if at(l) == Some("--------") {
                l += 1;
            }
            // `self.name = self.rawLines[l]` -- nil only for a text that ends right after the separator; that crashes
            // upstream on the first `self.name:...`; "?" keeps pob-ts' behaviour.
            self.name = at(l).unwrap_or("?").to_owned();
            let unidentified = raw_lines.iter().any(|x| x == "Unidentified");
            if !(self.rarity == "NORMAL" || self.rarity == "MAGIC" || unidentified) {
                l += 1;
            }
        }
        self.check_section = false;
        self.sockets = Vec::new();
        self.sockets_colour_always_matches = false;
        self.class_requirement_mod_lines = Vec::new();
        self.buff_mod_lines = Vec::new();
        self.enchant_mod_lines = Vec::new();
        self.scourge_mod_lines = Vec::new();
        self.implicit_mod_lines = Vec::new();
        self.explicit_mod_lines = Vec::new();
        self.crucible_mod_lines = Vec::new();
        self.advanced_copy = false;
        // Item.lua:504-506 (3dbca4463 #10374): only in-game advanced copy text (with "{ ... }" modifier headers) is re-sorted into
        // stat order; text written by PoB keeps its line order, which the ModRange entries of saved builds refer to.
        let mut has_modifier_headers = false;
        self.mod_magnitude_mods = Vec::new();
        let mut implicit_lines = 0.0;
        self.variant_list = None;
        self.version_list = None;
        self.allow_duplicate_variants = false;
        self.variant_groups = BTreeMap::new();
        self.uses_variant_groups = false;
        self.prefixes_limit = None;
        self.suffixes_limit = None;
        self.prefixes = Vec::new();
        self.suffixes = Vec::new();
        self.base_lines = IndexMap::new();
        self.rare_like_unique = None;
        self.requirements = Requirements {
            str: Some(0.0),
            dex: Some(0.0),
            int: Some(0.0),
            ..Requirements::default()
        };
        self.foulborn = false;
        self.vestigial = false;
        self.unimplemented = Vec::new();
        let mut mutated_lines: Option<&'static pob_data::wire::OrderedMap<String>> = None;
        let mut imported_level_req: Option<f64> = None;
        let mut flask_buff_lines: Option<Vec<String>> = None;
        let mut tincture_buff_lines: Option<Vec<String>> = None;
        let mut defer_jewel_radius_index_assignment = false;
        let mut stage = Stage::FindImplicit;
        let mut found_explicit = false;
        let mut found_implicit = false;
        let line_prefix = "";
        let line_postfix = "";
        let jewel_radius = pob_data::rules::rules().jewel_radii.get("3_16").expect("data.jewelRadii 3_16");

        while let Some(raw_line) = at(l) {
            let mut line = raw_line.to_owned();
            if line == "Veiled Prefix" || line == "Veiled Suffix" {
                self.veiled = true;
            }
            if let Some(pos) = flask_buff_lines.as_ref().and_then(|b| b.iter().position(|x| *x == line)) {
                flask_buff_lines.as_mut().expect("some").remove(pos);
            } else if let Some(pos) = tincture_buff_lines.as_ref().and_then(|b| b.iter().position(|x| *x == line)) {
                tincture_buff_lines.as_mut().expect("some").remove(pos);
            } else if line == "--------" {
                self.check_section = true;
            } else if line == "Split" {
                self.split = true;
            } else if line == "Mirrored" {
                self.mirrored = true;
            } else if line == "Corrupted" {
                self.corrupted = true;
            } else if line == "Fractured Item" {
                self.fractured = true;
            } else if line == "Synthesised Item" {
                self.synthesised = true;
            } else if matches(&line, "Foil Unique") {
                self.foil_type = Some(match1(&line, "%((.-)%)").unwrap_or_else(|| "Rainbow".into()));
            } else if let Some(i) = INFLUENCE_INFO.iter().position(|(_, d)| format!("{d} Item") == line) {
                self.influence[i] = true;
            } else if line == "Requirements:" {
                // nothing to do
            } else if matches(&line, "^%(%a+") || matches(&line, "^%(%d+%%? of ") {
                // Reminder text
                while at(l).is_some_and(|x| !matches(x, "%)$")) {
                    l += 1;
                }
            } else if self.base.is_some_and(|b| b.flask.is_some())
                && (matches(&line, "^Lasts .+ Seconds$")
                    || matches(&line, "^Consumes %d+ of %d+ Charges on use$")
                    || matches(&line, "^Currently has %d+ Charges$"))
            {
                // In-game flask state and base properties aren't modifier lines.
            } else if matches(&line, "^{ ") {
                // Item.lua:574-631: the in-game advanced copy format needs the affix database (data.itemMods) -- P5.
                // :578-580 the flags are set before the line is parsed
                self.advanced_copy = true;
                has_modifier_headers = true;
                self.unimplemented.push(format!("{ITEM}:577 advanced copy line skipped: {line}"));
            } else {
                line = format!("{line_prefix}{line}{line_postfix}");
                if self.check_section {
                    if stage == Stage::Implicit {
                        if found_implicit {
                            stage = Stage::Explicit;
                            found_explicit = true;
                        } else {
                            stage = Stage::FindExplicit;
                        }
                    } else if stage == Stage::Explicit {
                        stage = Stage::Done;
                    } else if stage == Stage::FindImplicit
                        && self.item_level.is_some()
                        && !matches(&line, " %(implicit%)")
                        && !matches(&line, " %(enchant%)")
                        && !find(&line, "Talisman Tier", ITEM)
                    {
                        stage = Stage::Explicit;
                        found_explicit = true;
                    }
                    self.check_section = false;
                }
                let (spec_name, spec_val) = match match_(&line, "^([%a %(%)]+:?): (.+)$", ITEM) {
                    Some(c) => {
                        let name = if c[0] == "Class:" {
                            "Requires Class".to_owned()
                        } else {
                            c[0].clone()
                        };
                        (Some(name), Some(c[1].clone()))
                    }
                    None => match match_(&line, "^(Requires %a+) (.+)$", ITEM) {
                        Some(c) => (Some(c[0].clone()), Some(c[1].clone())),
                        None => (None, None),
                    },
                };
                if let (Some(spec_name), Some(spec_val)) = (&spec_name, &spec_val) {
                    self.parse_spec(
                        spec_name,
                        spec_val,
                        jewel_radius,
                        &mut l,
                        &mut implicit_lines,
                        &mut stage,
                        &mut found_explicit,
                        &mut imported_level_req,
                        &mut defer_jewel_radius_index_assignment,
                    );
                }
                if line == "Prefixes:" {
                    found_explicit = true;
                    stage = Stage::Explicit;
                }
                if spec_name.is_none() || found_explicit || found_implicit {
                    let mut ml = ModLine::default();
                    let mut uses_groups = false;
                    let mut advanced = false;
                    line = gsub_fn(&line, "{(%a*):?([^}]*)}", None, |c| {
                        let (k, val) = (c[0].as_str(), c[1].as_str());
                        match k {
                            "variant" => {
                                ml.variant_list = Some(gmatch(val, "%d+").iter().filter_map(|d| tonumber(&d[0])).map(NumKey).collect());
                            }
                            "version" => {
                                ml.version_list = Some(gmatch(val, "%d+").iter().filter_map(|d| tonumber(&d[0])).map(NumKey).collect());
                                uses_groups = true;
                            }
                            "group" => {
                                ml.variant_group_list = Some(
                                    gmatch(val, "%d+")
                                        .iter()
                                        .filter_map(|d| tonumber(&d[0]))
                                        .filter(|g| *g > 0.0)
                                        .map(NumKey)
                                        .collect(),
                                );
                                uses_groups = true;
                            }
                            "tags" => {
                                for t in gmatch(val, "[%a_]+") {
                                    ml.mod_tags.push(t[0].clone());
                                }
                            }
                            "modGroup" => ml.mod_group = Some(val.to_owned()),
                            "range" => {
                                advanced = true;
                                ml.range = tonumber(val);
                            }
                            "corruptedRange" => ml.corrupted_range = tonumber(val),
                            _ => ml.set_flag(k),
                        }
                        Some(String::new())
                    });
                    if uses_groups {
                        self.uses_variant_groups = true;
                    }
                    if advanced {
                        self.advanced_copy = true;
                    }
                    if let (Some(groups), Some(variants)) = (&ml.variant_group_list, &ml.variant_list) {
                        for g in groups {
                            let n_variants = self.variant_list.as_ref().map_or(0, Vec::len);
                            let n_versions = self.version_list.as_ref().map_or(0, Vec::len);
                            let group = self.variant_groups.entry(*g).or_default();
                            for v in variants {
                                if v.0 >= 1.0 && v.0 <= n_variants as f64 && v.0.fract() == 0.0 {
                                    let versions = group.entry(*v).or_default();
                                    match &ml.version_list {
                                        Some(vl) => {
                                            for ver in vl {
                                                if ver.0 >= 1.0 && ver.0 <= n_versions as f64 && ver.0.fract() == 0.0 {
                                                    versions.insert(*ver);
                                                }
                                            }
                                        }
                                        None => {
                                            versions.insert(NumKey(0.0));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    line = gsub_fn(&line, " %((%l+)%)", None, |c| {
                        ml.set_flag(&c[0]);
                        Some(String::new())
                    });
                    if ml.flag("enchant") {
                        ml.set_flag("crafted");
                        ml.set_flag("implicit");
                    }
                    if ml.flag("vestigial") {
                        self.vestigial = true;
                    }
                    if matches(&line, "%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)") {
                        self.advanced_copy = true;
                    }
                    let mut base_name: Option<String> = None;
                    if self.base.is_none() && (self.rarity == "NORMAL" || self.rarity == "MAGIC") {
                        if matches(&self.name, "Energy Blade")
                            && let Some(ic) = &item_class
                        {
                            self.name = if matches(ic, "One Hand") {
                                "Energy Blade One Handed"
                            } else {
                                "Energy Blade Two Handed"
                            }
                            .into();
                        }
                        if item_bases().base(&self.name).is_some() {
                            base_name = Some(self.name.clone());
                        } else {
                            // D13 `pairs(data.itemBases)`: the longest plain match wins; byte order is the fixed tie-break.
                            let mut best: Option<(&str, usize, usize, usize)> = None;
                            for name in base_names_sorted() {
                                if let Some((s, e)) = find_plain(&self.name, name)
                                    && best.is_none_or(|b| (e - s) as i64 > b.1 as i64)
                                {
                                    best = Some((name, e - s, s, e));
                                }
                            }
                            if let Some((name, _, s, e)) = best {
                                self.name_prefix = self.name[..s - 1].to_owned();
                                self.name_suffix = self.name[e..].to_owned();
                                base_name = Some(name.to_owned());
                            }
                        }
                        if base_name.is_none()
                            && let Some((s, e)) = find_plain(&self.name, "Two-Toned Boots")
                        {
                            base_name = Some("Two-Toned Boots".into());
                            self.name_prefix = self.name[..s - 1].to_owned();
                            self.name_suffix = self.name[e..].to_owned();
                        }
                        self.name = gsub(&self.name, " %(.+%)", "", None);
                    }
                    let mut base_name = base_name.unwrap_or_else(|| {
                        let b = gsub(&line, "^Superior ", "", None);
                        let b = gsub(&b, "^Synthesised ", "", None);
                        gsub(&b, "^Vestigial ", "", None)
                    });
                    if base_name == "Two-Toned Boots" {
                        base_name = "Two-Toned Boots (Armour/Energy Shield)".into();
                    }
                    if let Some(base) = item_bases().base(&base_name) {
                        // Items with variants can have multiple bases (`self.baseLines[baseName]`, read by BuildRaw).
                        self.base_lines.insert(
                            base_name.clone(),
                            BaseLine {
                                variant_list: ml.variant_list.clone(),
                                version_list: ml.version_list.clone(),
                                variant_group_list: ml.variant_group_list.clone(),
                            },
                        );
                        if self.uses_variant_groups && self.version_list.is_some() && self.selected_version.is_none() {
                            self.selected_version = Some(self.version_list.as_ref().map_or(0, Vec::len) as f64);
                        }
                        let base_matches = if self.uses_variant_groups {
                            self.check_mod_line_variant(&ml)
                        } else {
                            self.variant.is_none() || ml.variant_list.as_ref().is_none_or(|v| set_has(v, self.variant))
                        };
                        if base_matches {
                            self.base_name = Some(base_name.clone());
                            if !(self.rarity == "NORMAL" || self.rarity == "MAGIC") {
                                self.title = Some(self.name.clone());
                            }
                            if self.rarity == "UNIQUE"
                                && let Some(title) = &self.title
                                && !find(&lower(title), "might of the meek", ITEM)
                            {
                                mutated_lines = foulborn_map().get(&gsub(title, "^[Ff]oulborn ", "", None));
                            }
                            self.ty = Some(base.ty.clone());
                            self.base = Some(base);
                            // Item.lua:1031-1038 (`enchantments` :1040-1046 feed the crafting UI only).
                            self.affixes = Some(crate::build_raw::item_affixes_for_base(base));
                            if let Some(title) = &self.title {
                                self.rare_like_unique = crate::build_raw::rare_like_unique(&lower(title));
                                if let Some(rlu) = self.rare_like_unique {
                                    self.affixes = Some(&rlu.affixes);
                                }
                            }
                            self.corruptible = base.ty != "Flask";
                            self.can_be_influenced = base.influence_tags.is_some();
                            self.cluster_jewel = pob_data::cluster_jewels::cluster_jewels().jewel(&base_name);
                            let (s, d, i) = (
                                base.req.str.unwrap_or(0.0),
                                base.req.dex.unwrap_or(0.0),
                                base.req.int.unwrap_or(0.0),
                            );
                            self.requirements.str = Some(s);
                            self.requirements.dex = Some(d);
                            self.requirements.int = Some(i);
                            let max_req = pob_lua::number::max(s, &[d, i]);
                            self.default_socket_color = Some(
                                if max_req == d {
                                    "G"
                                } else if max_req == i {
                                    "B"
                                } else {
                                    "R"
                                }
                                .to_owned(),
                            );
                            if let Some(buff) = base.flask.as_ref().and_then(|f| f.buff.as_ref())
                                && flask_buff_lines.is_none()
                            {
                                flask_buff_lines = Some(self.add_buff_lines(buff));
                            }
                            if base.tincture.is_some() && tincture_buff_lines.is_none() {
                                // `self.base.tincture.buff` -- the converter's tincture record has no `buff`.
                            }
                        }
                        l += 1;
                        continue; // Item.lua:1074 `goto continue`
                    }
                    if ml.flag("implicit") {
                        found_implicit = true;
                        stage = Stage::Implicit;
                    }
                    let mut catalyst_scalar = 1.0;
                    if matches(&line, " %- Unscalable Value$") {
                        line = gsub(&line, " %- Unscalable Value$", "", None);
                        ml.set_flag("unscalable");
                    } else {
                        catalyst_scalar = get_catalyst_scalar(self.catalyst, &ml, self.catalyst_quality);
                    }
                    line = gsub(&line, "(%-?%d+%.?%d*)%((%-?%d+%.?%d*)%)", "%1", None);
                    // Item.lua:1083 `pendingAffixList` exists only after an advanced-copy `{ ... }` header (skipped above).
                    line = resolve_advanced_copy_ranges(&line, &mut ml);
                    let mut ranged_line = apply_range(&line, 1.0, Some(catalyst_scalar), ml.corrupted_range, self.parse_mode);
                    let mut parsed = pob_parser::parse_mod_line(&ranged_line, false, self.parse_mode);
                    let next_mod_group = at(l + 1).and_then(|n| match1(n, "{modGroup:([^}]+)}"));
                    if (parsed.mod_list.is_none() || parsed.extra.is_some()) && at(l + 1).is_some() && ml.mod_group == next_mod_group {
                        let next = gsub(at(l + 1).expect("checked"), "%b{}", "", None);
                        let next_line = gsub(&next, " ?%(%l+%)", "", None);
                        let comb_line = format!("{line} {next_line}");
                        ranged_line = apply_range(&comb_line, 1.0, Some(catalyst_scalar), ml.corrupted_range, self.parse_mode);
                        parsed = pob_parser::parse_mod_line(&ranged_line, true, self.parse_mode);
                        if parsed.mod_list.is_some() && parsed.extra.is_none() {
                            line = format!("{line}\n{next_line}");
                            l += 1;
                        } else {
                            parsed = pob_parser::parse_mod_line(&ranged_line, false, self.parse_mode);
                        }
                    }
                    self.apply_line_flags(&ml, &line);
                    self.collect_mod_magnitude(&ml, &line, &ranged_line, catalyst_scalar);
                    let n_enchant = self.enchant_mod_lines.len() as f64;
                    let n_implicit = self.implicit_mod_lines.len() as f64;
                    let n_scourge = self.scourge_mod_lines.len() as f64;
                    let list = if ml.flag("enchant") || (ml.flag("crafted") && n_enchant + n_implicit < implicit_lines) {
                        ListId::Enchant
                    } else if ml.flag("scourge") {
                        ListId::Scourge
                    } else if find_plain(&line, "Requires Class").is_some() {
                        ListId::ClassRequirement
                    } else if ml.flag("implicit") || (!ml.flag("crafted") && n_enchant + n_scourge + n_implicit < implicit_lines) {
                        ListId::Implicit
                    } else if ml.flag("crucible") {
                        ListId::Crucible
                    } else {
                        ListId::Explicit
                    };
                    ml.line = line.clone();
                    if let Some(mods) = parsed.mod_list {
                        ml.mod_list = Some(mods);
                        ml.extra = parsed.extra;
                        ml.value_scalar = Some(catalyst_scalar);
                        ml.range = Some(ml.range.unwrap_or(DEFAULT_ITEM_AFFIX_QUALITY));
                        self.list_mut(list).push(ml);
                        if mode_game {
                            match stage {
                                Stage::FindImplicit => stage = Stage::Implicit,
                                Stage::FindExplicit => {
                                    found_explicit = true;
                                    stage = Stage::Explicit;
                                }
                                Stage::Explicit => found_explicit = true,
                                _ => {}
                            }
                        } else {
                            found_explicit = true;
                        }
                    } else if mode_game {
                        let base_line = self
                            .base
                            .is_some_and(|b| line == b.ty || b.sub_type.as_ref().is_some_and(|st| line == format!("{st} {}", b.ty)));
                        if stage == Stage::Implicit
                            || stage == Stage::Explicit
                            || (stage == Stage::FindImplicit
                                && item_bases().base(&line).is_none()
                                && self.name != line
                                && !matches(&line, "Two%-Toned")
                                && !base_line)
                        {
                            ml.mod_list = Some(Vec::new());
                            ml.extra = Some(line.clone()); // W16: `extra` is the line itself here
                            ml.range = Some(DEFAULT_ITEM_AFFIX_QUALITY);
                            self.list_mut(list).push(ml);
                        } else if stage == Stage::FindExplicit {
                            stage = Stage::Done;
                        }
                    } else if found_explicit || stage == Stage::Explicit {
                        ml.mod_list = Some(Vec::new());
                        ml.extra = Some(line.clone());
                        self.list_mut(list).push(ml);
                    }
                }
            }
            l += 1;
        }

        if has_modifier_headers && (self.rarity == "UNIQUE" || self.rarity == "RELIC") {
            // Item.lua:1360-1377 `modLine.order` from data.itemMods.ItemExclusive statOrder.
            let (exact, normalised) = unique_mod_stat_order();
            for ml in &mut self.explicit_mod_lines {
                let exact_line = gsub(
                    &lower(&ml.line),
                    "
",
                    " ",
                    None,
                );
                ml.order = exact
                    .get(&exact_line)
                    .or_else(|| normalised.get(&normalise_mod_line(&ml.line)))
                    .copied();
            }
        }
        if let Some(base) = self.base
            && !self.sockets.is_empty()
        {
            self.requirements.str = Some(base.req.str.unwrap_or(0.0));
            self.requirements.dex = Some(base.req.dex.unwrap_or(0.0));
            self.requirements.int = Some(base.req.int.unwrap_or(0.0));
        }
        if let Some(base) = self.base
            && self.requirements.level.is_none()
        {
            self.requirements.level = match imported_level_req {
                Some(lv) if self.sockets.is_empty() => Some(lv),
                _ => base.req.level,
            };
        }
        // hack for cane of kulemak
        if self.title.as_deref().map(lower).as_deref() == Some("cane of kulemak")
            && (self.rarity == "UNIQUE" || self.rarity == "RELIC")
            && self.advanced_copy
        {
            for m in &mut self.explicit_mod_lines {
                if !matches(&m.line, "magnitude") {
                    m.set_flag("unveiled");
                }
            }
        }
        if self.advanced_copy || self.crafted {
            self.apply_mod_magnitude();
        }
        if has_modifier_headers && self.explicit_mod_lines.len() > 1 {
            sort_crafted_mod_lines(&mut self.explicit_mod_lines);
        }
        // Item.lua:1474-1520 `affixLimit` and the prefix/suffix slots (written back by BuildRaw).
        self.affix_limit = 0.0;
        if self.crafted {
            if let Some(affixes) = self.affixes {
                let (max, min) = (pob_lua::number::max2, pob_lua::number::min2);
                if self.rarity == "MAGIC" {
                    if self.prefixes_limit.is_some() || self.suffixes_limit.is_some() {
                        let p = max(min(self.prefixes_limit.unwrap_or(0.0) + 1.0, 2.0), 0.0);
                        let s = max(min(self.suffixes_limit.unwrap_or(0.0) + 1.0, 2.0), 0.0);
                        (self.prefixes_limit, self.suffixes_limit) = (Some(p), Some(s));
                        self.affix_limit = p + s;
                    } else {
                        self.affix_limit = 2.0;
                    }
                } else if self.rarity == "RARE" {
                    let ty = self.ty.as_deref();
                    let jewel = ty == Some("Jewel") && {
                        let base = self
                            .base
                            .unwrap_or_else(|| panic!("{ITEM}:1487: attempt to index field 'base' (a nil value)"));
                        !(base.sub_type.as_deref() == Some("Abyss") && self.corrupted)
                    };
                    self.affix_limit = if jewel || ty == Some("Graft") { 4.0 } else { 6.0 };
                    if self.prefixes_limit.is_some() || self.suffixes_limit.is_some() {
                        let lim = self.affix_limit;
                        let p = max(min(self.prefixes_limit.unwrap_or(0.0) + lim / 2.0, lim), 0.0);
                        let s = max(min(self.suffixes_limit.unwrap_or(0.0) + lim / 2.0, lim), 0.0);
                        (self.prefixes_limit, self.suffixes_limit) = (Some(p), Some(s));
                        self.affix_limit = p + s;
                    }
                } else if let Some(rlu) = self.rare_like_unique {
                    (self.prefixes_limit, self.suffixes_limit) = (Some(rlu.prefix_limit), Some(rlu.suffix_limit));
                    self.affix_limit = rlu.prefix_limit + rlu.suffix_limit;
                } else {
                    self.crafted = false;
                }
                if self.crafted {
                    for (is_prefix, limit) in [(true, self.prefixes_limit), (false, self.suffixes_limit)] {
                        let n = limit.unwrap_or(self.affix_limit / 2.0);
                        let mut list = std::mem::take(if is_prefix { &mut self.prefixes } else { &mut self.suffixes });
                        let mut i = 1.0;
                        while i <= n {
                            let idx = i as usize - 1;
                            if idx >= list.len() {
                                // `list[i] = { modId = "None" }` -- `i` runs 1.., so the hole is always the next slot.
                                list.push(Affix {
                                    mod_id: "None".into(),
                                    range: None,
                                    fractured: false,
                                });
                            } else if list[idx].mod_id != "None" && !affixes.contains(&list[idx].mod_id) {
                                if let Some(id) = affixes.by_affix_name(&list[idx].mod_id, &mut self.unimplemented) {
                                    list[idx].mod_id = id;
                                }
                                if !affixes.contains(&list[idx].mod_id) {
                                    list[idx].mod_id = "None".into();
                                }
                            }
                            i += 1.0;
                        }
                        *(if is_prefix { &mut self.prefixes } else { &mut self.suffixes }) = list;
                    }
                }
            } else {
                // `not self.affixes` -- set together with the first matched base (Item.lua:1031), never reset by ParseRaw.
                self.crafted = false;
            }
        }
        if let Some(limit) = self.base.and_then(|b| b.socket_limit)
            && self.sockets.is_empty()
        {
            let mut i = 1.0;
            while i <= limit {
                self.sockets.push(Socket {
                    color: self.default_socket_color.clone().expect("defaultSocketColor"),
                    group: 0.0,
                });
                i += 1.0;
            }
        }
        self.abyssal_socket_count = 0.0;
        if self.uses_variant_groups {
            self.normalise_variant_selections();
        } else if let Some(vl) = &self.variant_list {
            let n = vl.len() as f64;
            self.variant = Some(pob_lua::number::min2(n, self.variant.unwrap_or(n)));
            for i in 0..5 {
                if self.has_alt_variant[i] {
                    self.variant_alt[i] = Some(pob_lua::number::min2(n, self.variant_alt[i].unwrap_or(n)));
                }
            }
        }
        if let Some(pairs) = mutated_lines {
            // Item.lua:1551-1600: match both sides so the same checkbox can apply or revert the transformation.
            for (orig_mod_id, foul_mod_id) in pairs.iter() {
                self.check_mutated_mod(orig_mod_id, foul_mod_id, false);
                self.check_mutated_mod(foul_mod_id, orig_mod_id, true);
            }
        }
        if self.explicit_mod_lines.iter().any(|v| v.flag("mutated")) {
            self.foulborn = true;
        }
        let has_foulborn_prefix = self.title.as_deref().is_some_and(|t| find(&lower(t), "^foulborn ", ITEM));
        if self.foulborn && !has_foulborn_prefix {
            let t = self
                .title
                .as_deref()
                .unwrap_or_else(|| panic!("{ITEM}:1607: attempt to concatenate field 'title' (a nil value)"));
            self.title = Some(format!("Foulborn {t}"));
        } else if !self.foulborn && has_foulborn_prefix {
            self.title = self.title.as_deref().map(|t| gsub(t, "[Ff]oulborn ", "", None));
        }
        if let (Some(base_name), Some(title)) = (&self.base_name, &self.title) {
            self.name = format!("{title}, {}", gsub(base_name, " %(.+%)", "", None));
        }
        if self.quality.is_none() {
            self.normalise_quality();
            if high_quality {
                self.normalise_quality();
            }
        }
        if let Some(enchant) = self.base.and_then(|b| b.enchant.as_ref())
            && self.enchant_mod_lines.is_empty()
        {
            let base = self.base.expect("checked");
            // :1626-1632 items saved before their base gained this enchant carry it as an implicit (Black Maw Talisman)
            let active_implicit_lines: std::collections::HashSet<String> = self
                .implicit_mod_lines
                .iter()
                .filter(|ml| self.check_mod_line_variant(ml))
                .map(|ml| ml.line.clone())
                .collect();
            for (idx, c) in gmatch(enchant, "[^\n]+").into_iter().enumerate() {
                let line = c[0].clone();
                if active_implicit_lines.contains(&line) {
                    continue;
                }
                let p = pob_parser::parse_mod_line(&line, false, self.parse_mode);
                let mut ml = ModLine {
                    line,
                    extra: p.extra,
                    mod_list: Some(p.mod_list.unwrap_or_default()),
                    mod_tags: base
                        .enchant_mod_types
                        .as_ref()
                        .and_then(|t| t.get(idx).cloned())
                        .unwrap_or_default(),
                    ..ModLine::default()
                };
                ml.set_flag("crafted");
                ml.set_flag("implicit");
                ml.set_flag("enchant");
                ml.added_from_base = true;
                self.enchant_mod_lines.push(ml);
            }
        }
        self.build_mod_list();
        if defer_jewel_radius_index_assignment {
            let jd = self
                .jewel_data
                .as_ref()
                .unwrap_or_else(|| panic!("{ITEM}:1641: attempt to index field 'jewelData' (a nil value)"));
            self.jewel_radius_index = jd.num("radiusIndex");
        }
        self.is_unique = self.rarity == "UNIQUE" || self.rarity == "RELIC";
    }

    /// Item.lua:1052-1067 -- the base's flask buff lines become `buffModLines`; returns the skip-set.
    fn add_buff_lines(&mut self, buff: &[String]) -> Vec<String> {
        let mut set = Vec::new();
        for line in buff {
            set.push(line.clone());
            let p = pob_parser::parse_mod_line(line, false, self.parse_mode);
            self.buff_mod_lines.push(ModLine {
                line: line.clone(),
                extra: p.extra,
                mod_list: Some(p.mod_list.unwrap_or_default()),
                ..ModLine::default()
            });
        }
        set
    }

    /// Item.lua:658-852 -- one `Name: value` spec line.
    #[allow(clippy::too_many_arguments)]
    fn parse_spec(
        &mut self,
        spec_name: &str,
        spec_val: &str,
        jewel_radius: &[pob_data::rules::JewelRadius],
        l: &mut usize,
        implicit_lines: &mut f64,
        stage: &mut Stage,
        found_explicit: &mut bool,
        imported_level_req: &mut Option<f64>,
        defer_jewel_radius: &mut bool,
    ) {
        let num = || spec_to_number(spec_val);
        match spec_name {
            "Unique ID" => self.unique_id = Some(spec_val.to_owned()),
            "Item Level" => self.item_level = num(),
            "Memory Strands" => self.memory_strands = num(),
            "Requires Class" => self.class_restriction = Some(spec_val.to_owned()),
            _ if matches(spec_name, "Quality %([%a%s]+ Modifiers%)") => {
                self.catalyst_quality = match1(spec_val, "(%d+)%%").and_then(|q| spec_to_number(&q));
                let desc = match1(spec_name, "Quality %(([%a%s]+) Modifiers%)");
                for (i, d) in CATALYST_DESCRIPTOR_LIST.iter().enumerate() {
                    if desc.as_deref() == Some(*d) {
                        self.catalyst = Some((i + 1) as f64);
                    }
                }
            }
            "Quality" => self.quality = num(),
            "Sockets" => {
                let mut group = 0.0;
                for c in spec_val.bytes() {
                    if b"RGBWA".contains(&c) {
                        self.sockets.push(Socket {
                            color: (c as char).to_string(),
                            group,
                        });
                    } else if c == b' ' {
                        group += 1.0;
                    }
                }
            }
            "Radius" if self.ty.as_deref() == Some("Jewel") => {
                let label = match1(spec_val, "^[%a ]+");
                self.jewel_radius_label = label.clone();
                if label.as_deref() == Some("Variable") {
                    *defer_jewel_radius = true;
                } else {
                    // MINEFIELD D9 (3): `data.jewelRadius` is the LATEST tree's table while items load (Build.lua:606).
                    for (index, data) in jewel_radius.iter().enumerate() {
                        if label.as_deref() == Some(data.label.as_str()) {
                            self.jewel_radius_index = Some((index + 1) as f64);
                            break;
                        }
                    }
                }
            }
            "Limited to" if self.ty.as_deref() == Some("Jewel") => self.limit = num(),
            "Version" => {
                self.version_list.get_or_insert_default().push(spec_val.to_owned());
                self.uses_variant_groups = true;
            }
            "Variant" => {
                let v = match match_(spec_val, "{([%w_]+)}(.+)", ITEM) {
                    Some(c) => c[1].clone(),
                    None => spec_val.to_owned(),
                };
                self.variant_list.get_or_insert_default().push(v);
            }
            "Talisman Tier" => self.talisman_tier = num(),
            "Armour" | "Evasion Rating" | "Evasion" | "Energy Shield" | "Ward" => {
                let key = match spec_name {
                    "Evasion Rating" => {
                        if self.base_name.as_deref() == Some("Two-Toned Boots (Armour/Energy Shield)") {
                            self.base_name = Some("Two-Toned Boots (Armour/Evasion)".into());
                            self.base = item_bases().base("Two-Toned Boots (Armour/Evasion)");
                        }
                        "Evasion"
                    }
                    "Energy Shield" => {
                        if self.base_name.as_deref() == Some("Two-Toned Boots (Armour/Evasion)") {
                            self.base_name = Some("Two-Toned Boots (Evasion/Energy Shield)".into());
                            self.base = item_bases().base("Two-Toned Boots (Evasion/Energy Shield)");
                        }
                        "EnergyShield"
                    }
                    other => other,
                };
                let v = num();
                self.armour_data.get_or_insert_default().set(key, Value::from(v));
            }
            _ if matches(spec_name, "BasePercentile") => {
                let v = num().unwrap_or(0.0);
                self.armour_data.get_or_insert_default().set_num(spec_name, v);
            }
            "Requires Level" => self.requirements.level = num(),
            "Level" => *imported_level_req = num(),
            "LevelReq" => self.requirements.level = num(),
            "Has Alt Variant" => self.has_alt_variant[0] = true,
            "Has Alt Variant Two" => self.has_alt_variant[1] = true,
            "Has Alt Variant Three" => self.has_alt_variant[2] = true,
            "Has Alt Variant Four" => self.has_alt_variant[3] = true,
            "Has Alt Variant Five" => self.has_alt_variant[4] = true,
            "Selected Version" => {
                self.selected_version = num();
                self.uses_variant_groups = true;
            }
            "Selected Variant Group" => {
                if let Some(c) = match_(spec_val, "^(%d+)%s*=%s*(%d+)$", ITEM)
                    && let Some(g) = tonumber(&c[0])
                {
                    match tonumber(&c[1]) {
                        Some(v) => self.variant_group_selections.insert(NumKey(g), v),
                        None => self.variant_group_selections.remove(&NumKey(g)),
                    };
                    self.uses_variant_groups = true;
                }
            }
            "Selected Variant" => self.variant = num(),
            "Selected Alt Variant" => self.variant_alt[0] = num(),
            "Selected Alt Variant Two" => self.variant_alt[1] = num(),
            "Selected Alt Variant Three" => self.variant_alt[2] = num(),
            "Selected Alt Variant Four" => self.variant_alt[3] = num(),
            "Selected Alt Variant Five" => self.variant_alt[4] = num(),
            "Allow Duplicate Variants" => self.allow_duplicate_variants = spec_val == "true",
            "Has Variants" | "Selected Variants" => *l += 1,
            "League" => self.league = Some(spec_val.to_owned()),
            "Crafted" => self.crafted = true,
            "Scourge" => self.scourge = true,
            "Crucible" => self.crucible = true,
            "Implicit" => self.implicit = true,
            "Prefix" | "Suffix" => {
                // Item.lua:799-819.
                let fractured = matches(spec_val, "^{fractured}");
                let spec_val = gsub(spec_val, "^{fractured}", "", None);
                let (range, affix) = match match_(&spec_val, "{range:([^}]+)}(.+)", ITEM) {
                    Some(c) => (Some(c[0].clone()), Some(c[1].clone())),
                    None => (None, None),
                };
                let mut range = match &range {
                    Some(r) if find_plain(r, ",").is_some() => Some(AffixRange::List(
                        gmatch(r, "[^,]+").iter().filter_map(|v| tonumber(&v[0])).collect(),
                    )),
                    Some(r) => tonumber(r).map(AffixRange::Num),
                    None => None,
                };
                let mod_id = affix.unwrap_or(spec_val);
                if range.is_none() && mod_id != "None" {
                    range = Some(AffixRange::Num(DEFAULT_ITEM_AFFIX_QUALITY));
                }
                let affixes = if spec_name == "Prefix" {
                    &mut self.prefixes
                } else {
                    &mut self.suffixes
                };
                affixes.push(Affix { mod_id, range, fractured });
            }
            "Implicits" => {
                *implicit_lines = num().unwrap_or(0.0);
                *stage = Stage::Explicit;
            }
            "Unreleased" => self.unreleased = Some(spec_val == "true"),
            "Upgrade" => self.upgrade_paths.get_or_insert_default().push(spec_val.to_owned()),
            "Source" => self.source = Some(spec_val.to_owned()),
            "Cluster Jewel Skill" => {
                if self.cluster_jewel.is_some_and(|c| c.skills.contains_key(spec_val)) {
                    self.cluster_jewel_skill = Some(spec_val.to_owned());
                }
            }
            "Cluster Jewel Node Count" => {
                if let Some(cj) = self.cluster_jewel {
                    let n = num().unwrap_or(cj.max_nodes);
                    self.cluster_jewel_node_count = Some(pob_lua::number::min2(pob_lua::number::max2(n, cj.min_nodes), cj.max_nodes));
                }
            }
            "Catalyst" => {
                for (i, c) in CATALYST_LIST.iter().enumerate() {
                    if spec_val == *c {
                        self.catalyst = Some((i + 1) as f64);
                    }
                }
            }
            "CatalystQuality" => self.catalyst_quality = num(),
            "Intangibility" => self.intangibility = num(),
            "Note" => self.note = Some(spec_val.to_owned()),
            "Str" | "Strength" => self.requirements.str = num(),
            "Dex" | "Dexterity" => self.requirements.dex = num(),
            "Int" | "Intelligence" => self.requirements.int = num(),
            "Critical Strike Range"
            | "Attacks per Second"
            | "Weapon Range"
            | "Critical Strike Chance"
            | "Physical Damage"
            | "Elemental Damage"
            | "Chaos Damage"
            | "Chance to Block"
            | "Block chance" => self.hidden_specs = true,
            _ => {
                // An explicit with a colon in it (Fortress Covenant, Pure Talent, ...) unless it is part of the custom name.
                // Both are Lua PATTERNS here (upstream quirk); a malformed one is the Lua error.
                let site = "Classes/Item.lua:861";
                if !(find(&self.name, spec_name, site) && find(&self.name, spec_val, site)) {
                    *found_explicit = true;
                    *stage = Stage::Explicit;
                }
            }
        }
    }

    /// Item.lua:1196-1240 -- the plain "this line means X about the item" flags.
    fn apply_line_flags(&mut self, ml: &ModLine, line: &str) {
        let ll = if ml.flag("disabled") { String::new() } else { lower(line) };
        let limit = |lim: Option<f64>, word: &str| -> f64 {
            lim.unwrap_or(0.0)
                + match1(&ll, &format!("%+(%d+) {word} modifiers? allowed"))
                    .and_then(|x| tonumber(&x))
                    .unwrap_or(0.0)
                - match1(&ll, &format!("%-(%d+) {word} modifiers? allowed"))
                    .and_then(|x| tonumber(&x))
                    .unwrap_or(0.0)
        };
        if ll == "implicit modifiers cannot be changed" {
            self.implicits_cannot_be_changed = true;
        } else if matches(&ll, " prefix modifiers? allowed") {
            self.prefixes_limit = Some(limit(self.prefixes_limit, "prefix"));
        } else if matches(&ll, " suffix modifiers? allowed") {
            self.suffixes_limit = Some(limit(self.suffixes_limit, "suffix"));
        } else if find(&ll, "can be anointed", ITEM) {
            self.can_be_anointed = true;
        } else if ll == "can have a second enchantment modifier" || ll == "can have 1 additional enchantment modifiers" {
            self.can_have_two_enchants = true;
        } else if ll == "can have 2 additional enchantment modifiers" {
            self.can_have_two_enchants = true;
            self.can_have_three_enchants = true;
        } else if ll == "can have 3 additional enchantment modifiers" {
            self.can_have_two_enchants = true;
            self.can_have_three_enchants = true;
            self.can_have_four_enchants = true;
        } else if ll == "has elder, shaper and all conqueror influences" {
            self.has_elder_shaper_and_all_conqueror_influences = true;
            for i in 0..6 {
                self.influence[i] = true;
            }
        } else if matches(&ll, "if the eater of worlds is dominant") {
            self.can_have_eldritch_influence = true;
        } else if ll == "has a crucible passive skill tree with only support passive skills" {
            self.can_have_only_support_skills_crucible_tree = true;
        } else if ll == "has a crucible passive skill tree" {
            self.can_have_shield_crucible_tree = true;
        } else if ll == "has a two handed sword crucible passive skill tree" {
            self.can_have_two_handed_sword_crucible_tree = true;
        } else if ll == "cannot roll caster modifiers" {
            self.restrict_tag = true;
            self.no_caster = true;
        } else if ll == "cannot roll attack modifiers" {
            self.restrict_tag = true;
            self.no_attack = true;
        } else {
            for (i, dt) in crate::craft::DAMAGE_TYPES.iter().enumerate() {
                if ll == format!("cannot roll modifiers of non-{dt} damage types") {
                    self.restrict_damage_type = true;
                    self.only_damage_type[i] = true;
                }
            }
        }
    }

    /// Item.lua:1197-1203 + 1241-1295 -- "X% increased <tags> modifier magnitudes" lines.
    fn collect_mod_magnitude(&mut self, ml: &ModLine, line: &str, ranged_line: &str, catalyst_scalar: f64) {
        // (pattern, a literal every match of it contains) -- the literal is only a pre-filter: without it the pattern
        // cannot match, so skipping the unanchored `[%a%s]*` scan (quadratic in the line) changes nothing.
        const PATTERNS: [(&str, &str); 3] = [
            (
                "(%d+)%% ([ir][ne][cd][ru][ec][ae][sd]e?d?) ?([%a%s]*) modifier magnitudes",
                " modifier magnitudes",
            ),
            (
                "(%d+)%% ([ir][ne][cd][ru][ec][ae][sd]e?d?) effect of ([sp][ur][fe]fix)es",
                " effect of ",
            ),
            ("([%a%s]*) modifier magnitudes are doubled", " modifier magnitudes are doubled"),
        ];
        if ml.flag("disabled") {
            return;
        }
        let lowered = lower(ranged_line);
        for (pattern, literal) in PATTERNS {
            if !lowered.contains(literal) || !find(&lowered, pattern, ITEM) {
                continue;
            }
            let rl = apply_range(
                line,
                ml.range.unwrap_or(DEFAULT_ITEM_AFFIX_QUALITY),
                Some(catalyst_scalar),
                ml.corrupted_range,
                self.parse_mode,
            );
            let Some(c) = match_(&lower(&rl), pattern, ITEM) else { continue };
            let (mut amount, mut inc_or_dec, mut tags_string) = (c.first().cloned(), c.get(1).cloned(), c.get(2).cloned());
            let mut multiplier = None;
            if amount.is_some() && inc_or_dec.is_none() && tags_string.is_none() {
                tags_string = amount;
                amount = Some("100".into());
                inc_or_dec = Some("increased".into());
                multiplier = Some(2.0);
            }
            let (Some(amount), Some(tags_string)) = (amount, tags_string) else {
                continue;
            };
            let inc = match inc_or_dec.as_deref() {
                Some("increased") => true,
                Some("reduced") => false,
                _ => continue,
            };
            let a = tonumber(&amount).expect("amount");
            let quality = if inc { a } else { -a };
            if tags_string == "explicit physical and chaos damage" {
                self.mod_magnitude_mods.push(ModMagnitudeMod {
                    tags: vec!["damage".into()],
                    any_tags: Some(["physical".into(), "chaos".into()]),
                    quality,
                    multiplier: None,
                    mod_type: Some("explicit".into()),
                });
            } else {
                let mut tags = Vec::new();
                let mut mod_type = None;
                for w in gmatch(&format!("{tags_string} "), "%S+") {
                    let w = lower(&w[0]);
                    let w = match w.as_str() {
                        "defence" => "defences".to_owned(),
                        "enchantment" => "enchant".to_owned(),
                        _ => w,
                    };
                    if w == "implicit" || w == "explicit" || w == "enchant" {
                        mod_type = Some(w);
                    } else {
                        tags.push(w);
                    }
                }
                self.mod_magnitude_mods.push(ModMagnitudeMod {
                    tags,
                    any_tags: None,
                    quality,
                    multiplier,
                    mod_type,
                });
            }
            break;
        }
    }

    /// Item.lua:1401-1468 -- apply the collected mod magnitude boosts.
    fn apply_mod_magnitude(&mut self) {
        let mags = self.mod_magnitude_mods.clone();
        for mag in &mags {
            let lists: Vec<ListId> = match mag.mod_type.as_deref() {
                Some("implicit") => vec![ListId::Implicit],
                Some("explicit") => vec![ListId::Explicit],
                Some("enchant") => vec![ListId::Enchant],
                Some(_) => vec![],
                None => vec![ListId::Implicit, ListId::Explicit, ListId::Enchant],
            };
            for lid in lists {
                for idx in 0..self.list(lid).len() {
                    let m = &self.list(lid)[idx];
                    if m.variant_list.is_some() && self.get_mod_line_variant_count(m) == 0 {
                        continue;
                    }
                    let mut grants_skill = m.mod_list.as_ref().is_some_and(|l| l.iter().any(|p| &*p.name == "ExtraSkill"));
                    if m.extra.is_some() && !grants_skill {
                        let ll = lower(&m.line);
                        grants_skill = matches(&ll, "^grants level %d+ ") || matches(&ll, "^grants %D+$");
                    }
                    let has = |t: &str| m.mod_tags.iter().any(|x| x == t) || (matches!(t, "unveiled" | "prefix" | "suffix") && m.flag(t));
                    let mut matched = mag.tags.iter().all(|t| has(t));
                    if let Some([a, b]) = &mag.any_tags
                        && !(has(a) || has(b))
                    {
                        matched = false;
                    }
                    let unscalable = m.flag("unscalable");
                    let mode = self.parse_mode;
                    let m = &mut self.list_mut(lid)[idx];
                    if matched && !unscalable && !grants_skill {
                        m.value_scalar = Some(match mag.multiplier {
                            Some(mult) => m.value_scalar.unwrap_or(1.0) * mult,
                            None => m.value_scalar.unwrap_or(1.0) + mag.quality / 100.0,
                        });
                    }
                    if let Some(vs) = m.value_scalar
                        && vs != 1.0
                    {
                        let rl = apply_range(&m.line, m.range.unwrap_or(1.0), Some(vs), Some(1.0), mode);
                        let p = pob_parser::parse_mod_line(&rl, false, mode);
                        if let Some(mods) = p.mod_list {
                            m.display_value_scalar = Some(1.0);
                            m.mod_list = Some(mods);
                            m.extra = p.extra;
                        }
                    }
                }
            }
        }
    }

    // --- Item.lua:2519-2704 ---------------------------------------------------------------------------------------

    /// Item.lua:2519-2531 `getRangedModList`: `None` = Lua nil/false (keep the parsed list).
    fn get_ranged_mod_list(ml: &ModLine, mode: pob_parser::ParseMode) -> Option<Vec<Mod>> {
        let range = ml.range?;
        if !matches(&ml.line, "%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)") {
            return None;
        }
        let line = apply_range(&gsub(&ml.line, "\n", " ", None), range, ml.value_scalar, ml.corrupted_range, mode);
        let p = pob_parser::parse_mod_line(&line, false, mode);
        if is_zero_value_line(&line) {
            return Some(Vec::new());
        }
        if p.extra.is_some() { None } else { p.mod_list }
    }

    /// Item.lua:2533-2704 `BuildModList`.
    pub fn build_mod_list(&mut self) {
        let Some(base) = self.base else { return };
        let mut base_list: Vec<Mod> = Vec::new();
        if base.weapon.is_some() {
            self.weapon_data = Some(BTreeMap::new());
        } else if base.armour.is_some() {
            self.armour_data.get_or_insert_default();
        } else if base.flask.is_some() {
            self.flask_data = Some(DataTable::default());
            self.buff_mod_list = Some(Vec::new());
        } else if base.tincture.is_some() {
            self.tincture_data = Some(DataTable::default());
            self.buff_mod_list = Some(Vec::new());
        } else if self.ty.as_deref() == Some("Jewel") {
            self.jewel_data = Some(DataTable::default());
        }
        self.range_line_count = 0;
        let mod_source = format!("Item:{}:{}", tostring(self.id.unwrap_or(-1.0)), self.name);
        self.mod_source = Some(mod_source.clone());
        for idx in 0..self.buff_mod_lines.len() {
            let ml = &self.buff_mod_lines[idx];
            if ml.extra.is_none() && self.check_mod_line_variant(ml) {
                let ml = &mut self.buff_mod_lines[idx];
                for m in ml.mod_list.iter_mut().flatten() {
                    m.source = Some(Rc::from(mod_source.as_str()));
                    self.buff_mod_list.get_or_insert_default().push(m.clone());
                }
            }
        }
        for lid in [
            ListId::Enchant,
            ListId::Scourge,
            ListId::ClassRequirement,
            ListId::Implicit,
            ListId::Explicit,
            ListId::Crucible,
        ] {
            for idx in 0..self.list(lid).len() {
                let ml = &self.list(lid)[idx];
                if ml.flag("disabled") {
                    continue;
                }
                let variant_count = self.get_mod_line_variant_count(ml);
                if variant_count == 0 {
                    continue;
                }
                let class_restriction = find_plain(&ml.line, "Requires Class")
                    .is_some()
                    .then(|| match1(&gsub(&ml.line, "{variant:([%d,]+)}", "", None), "Requires Class (.+)"));
                let ranged = if ml.extra.is_none() {
                    Self::get_ranged_mod_list(ml, self.parse_mode)
                } else {
                    None
                };
                let has_mod_ids = ml.mod_id.is_some() && ml.new_mod_id.is_some();
                if let Some(cr) = class_restriction {
                    self.class_restriction = cr;
                }
                if ranged.is_some() || has_mod_ids {
                    self.range_line_count += 1;
                }
                let mut has_tags = false;
                let ml = &mut self.list_mut(lid)[idx];
                if let Some(r) = ranged {
                    ml.mod_list = Some(r);
                    ml.show_slider = Some(true);
                }
                if ml.extra.is_none() {
                    for m in ml.mod_list.iter_mut().flatten() {
                        set_source(m, &mod_source);
                        for _ in 0..variant_count {
                            base_list.push(m.clone());
                        }
                    }
                    has_tags = !ml.mod_tags.is_empty();
                }
                if has_tags {
                    self.has_mod_tags = true;
                }
            }
        }
        if matches!(
            self.name.as_str(),
            "Tabula Rasa, Simple Robe"
                | "Skin of the Loyal, Simple Robe"
                | "Skin of the Lords, Simple Robe"
                | "The Apostate, Cabalist Regalia"
        ) {
            let mut v = Table::default();
            v.hash.insert(Rc::from("key"), Value::str("EnergyShield"));
            v.hash.insert(Rc::from("value"), Value::Num(0.0));
            base_list.push(create_mod("ArmourData", Some(ModType::List), Value::table(v), None, 0, 0, vec![]));
            self.requirements.int = Some(0.0);
        }
        if calc_local_flag(&mut base_list, "NoAttributeRequirements", 0) {
            self.requirements.str_mod = Some(0.0);
            self.requirements.dex_mod = Some(0.0);
            self.requirements.int_mod = Some(0.0);
        } else {
            let site = "Classes/Item.lua:2619";
            let s = self
                .requirements
                .str
                .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on field 'str' (a nil value)"));
            let b = calc_local(&mut base_list, "StrRequirement", ModType::Base, 0);
            self.requirements.str_mod =
                Some(((s + b) * (1.0 + calc_local(&mut base_list, "StrRequirement", ModType::Inc, 0) / 100.0)).floor());
            let d = self
                .requirements
                .dex
                .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on field 'dex' (a nil value)"));
            let b = calc_local(&mut base_list, "DexRequirement", ModType::Base, 0);
            self.requirements.dex_mod =
                Some(((d + b) * (1.0 + calc_local(&mut base_list, "DexRequirement", ModType::Inc, 0) / 100.0)).floor());
            let i = self
                .requirements
                .int
                .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on field 'int' (a nil value)"));
            let b = calc_local(&mut base_list, "IntRequirement", ModType::Base, 0);
            self.requirements.int_mod =
                Some(((i + b) * (1.0 + calc_local(&mut base_list, "IntRequirement", ModType::Inc, 0) / 100.0)).floor());
        }
        self.granted_skills = Vec::new();
        for skill in list_values(&base_list, "ExtraSkill") {
            if !skill.get("name").is_str("Unknown") {
                self.granted_skills.push(GrantedSkill {
                    skill_id: skill.get("skillId"),
                    level: skill.get("level"),
                    no_supports: skill.get("noSupports"),
                    source: mod_source.clone(),
                    triggered: skill.get("triggered"),
                    trigger_chance: skill.get("triggerChance"),
                });
            }
        }
        let socket_count = calc_local(&mut base_list, "SocketCount", ModType::Base, 0);
        self.abyssal_socket_count = calc_local(&mut base_list, "AbyssalSocketCount", ModType::Base, 0);
        self.selectable_socket_count =
            Some(pob_lua::number::max2(base.socket_limit.unwrap_or(0.0), self.sockets.len() as f64) - self.abyssal_socket_count);
        if calc_local_flag(&mut base_list, "NoSockets", 0) {
            self.sockets.clear();
            self.selectable_socket_count = Some(0.0);
            self.abyssal_socket_count = 0.0;
        } else if socket_count > 0.0 {
            self.selectable_socket_count = Some(socket_count);
            let mut group = 0.0;
            let n = pob_lua::number::max2(socket_count, self.sockets.len() as f64);
            let mut i = 1.0;
            while i <= n {
                let k = i as usize;
                if i > socket_count {
                    self.sockets.truncate(k - 1);
                } else if k > self.sockets.len() {
                    self.sockets.push(Socket {
                        color: self.default_socket_color.clone().expect("defaultSocketColor"),
                        group,
                    });
                } else {
                    group = self.sockets[k - 1].group;
                }
                i += 1.0;
            }
        } else if self.abyssal_socket_count > 0.0 {
            let mut new_sockets: Vec<Socket> = Vec::new();
            let mut group = 0.0;
            let selectable = self.selectable_socket_count.expect("set above");
            for s in &self.sockets {
                if s.color != "A" {
                    if new_sockets.len() as f64 >= selectable {
                        break;
                    }
                    new_sockets.push(s.clone());
                    group = s.group;
                }
            }
            let mut i = 1.0;
            while i <= self.abyssal_socket_count {
                group += 1.0;
                new_sockets.push(Socket { color: "A".into(), group });
                i += 1.0;
            }
            self.sockets = new_sockets;
        }
        if calc_local_flag(&mut base_list, "SocketAlwaysMatches", 0) {
            self.sockets_colour_always_matches = true;
        }
        self.socketed_jewel_effect_modifier = Some(1.0 + calc_local(&mut base_list, "SocketedJewelEffect", ModType::Inc, 0) / 100.0);
        if base.weapon.is_some() || self.ty.as_deref() == Some("Ring") {
            let mut slots = vec![
                self.build_mod_list_for_slot_num(&mut base_list, Some(1.0)),
                self.build_mod_list_for_slot_num(&mut base_list, Some(2.0)),
            ];
            if self.ty.as_deref() == Some("Ring") {
                slots.push(self.build_mod_list_for_slot_num(&mut base_list, Some(3.0)));
            }
            self.slot_mod_list = Some(slots);
        } else {
            self.mod_list = Some(self.build_mod_list_for_slot_num(&mut base_list, None));
        }
        self.base_mod_list = Some(base_list);
    }

    /// Item.lua:2222-2517 `BuildModListForSlotNum`.
    /// `base_list` is mutable because the flask branch runs `calcLocal(baseList, ...)` (Item.lua:2440, :2447), which removes
    /// the matched mods from the item's `baseModList`.
    pub fn build_mod_list_for_slot_num(&mut self, base_list: &mut Vec<Mod>, slot_num: Option<f64>) -> Vec<SlotMod> {
        let base = self.base.expect("BuildModListForSlotNum needs a base");
        let mut slot_name = self.get_primary_slot();
        if slot_num != Some(1.0) {
            // MINEFIELD D18: `slotName:gsub("1", tostring(slotNum))` -- a nil slotNum gives "Flask nil" / "Graft nil".
            let repl = slot_num.map_or_else(|| "nil".to_owned(), tostring);
            slot_name = gsub(&slot_name, "1", &repl, None);
        }
        let hand = if slot_num == Some(1.0) { "MainHand" } else { "OffHand" };
        let other = if slot_num == Some(1.0) { "2" } else { "1" };
        let mut mod_list: Vec<SlotMod> = Vec::new();
        for base_mod in base_list.iter() {
            // `copyTable(baseMod)`: new mod, tag and value tables (MINEFIELD-RUST R4)
            let mut m = pob_parser::copy_table_mod(base_mod);
            let mut add = true;
            let mut tags = Vec::with_capacity(m.tags.len());
            for t in &m.tags {
                let ty = t.ty();
                if (ty.is_str("SlotNumber") || ty.is_str("InSlot")) && !t.get("num").raw_eq(&Value::from(slot_num)) {
                    add = false;
                    break;
                }
                let table = t.table();
                let mut nt = (*table).clone();
                for (_, v) in nt.hash.iter_mut() {
                    if let Value::Str(s) = v {
                        let x = gsub(s, "{SlotName}", &slot_name, None);
                        let x = gsub(&x, "{Hand}", hand, None);
                        let x = gsub(&x, "{OtherSlotNum}", other, None);
                        *v = Value::str(&x);
                    }
                }
                tags.push(Tag::new(nt));
            }
            if add {
                m.tags = tags;
                mod_list.push(SlotMod {
                    m,
                    source_slot: Some(Rc::from(slot_name.as_str())),
                });
            }
        }
        let new_mod = |list: &mut Vec<SlotMod>, name: &str, ty: ModType, value: Value, source: Option<&str>, tags: Vec<Tag>| {
            list.push(SlotMod {
                m: create_mod(name, Some(ty), value, source, 0, 0, tags),
                source_slot: None,
            });
        };
        if !self.sockets.is_empty() {
            let mut group_counts: BTreeMap<NumKey, u32> = BTreeMap::new();
            for s in &self.sockets {
                *group_counts.entry(NumKey(s.group)).or_default() += 1;
                let name = match s.color.as_str() {
                    "R" => Some("Red"),
                    "G" => Some("Green"),
                    "B" => Some("Blue"),
                    "W" => Some("White"),
                    _ => None,
                };
                if let Some(c) = name {
                    new_mod(
                        &mut mod_list,
                        &format!("Multiplier:{c}SocketIn{slot_name}"),
                        ModType::Base,
                        Value::Num(1.0),
                        Some("Item Sockets"),
                        vec![],
                    );
                }
            }
            let unlinked = group_counts.values().filter(|c| **c == 1).count() as f64;
            new_mod(
                &mut mod_list,
                &format!("Multiplier:UnlinkedSocketIn{slot_name}"),
                ModType::Base,
                Value::Num(unlinked),
                Some("Unlinked Item Sockets"),
                vec![],
            );
        }
        // `calcLocal(...) or 0` -- calcLocal of a BASE always returns a number.
        let crafted_quality = calc_local_slot(&mut mod_list, "Quality", ModType::Base, 0);
        if Some(crafted_quality) != self.crafted_quality {
            if let Some(cq) = self.crafted_quality {
                self.quality = Some(self.quality.unwrap_or(0.0) - cq + crafted_quality);
            }
            self.crafted_quality = Some(crafted_quality);
        }
        let cic = calc_local_slot(&mut mod_list, "CorruptImplicitCount", ModType::Base, 0);
        self.corrupt_implicit_count = if cic == 0.0 { None } else { Some(cic) };
        if let Some(q) = self.quality {
            new_mod(
                &mut mod_list,
                &format!("Multiplier:QualityOn{slot_name}"),
                ModType::Base,
                Value::Num(q),
                Some("Quality"),
                vec![],
            );
        }
        let mod_source = self.mod_source.clone();
        if let Some(weapon) = &base.weapon {
            let slot = slot_num.unwrap_or_else(|| panic!("{ITEM}:2276: table index is nil"));
            let mut wd = DataTable::default();
            wd.set("type", Value::str(&base.ty));
            wd.set("subType", base.sub_type.as_deref().map_or(Value::Nil, Value::str));
            wd.set("name", Value::str(&self.name));
            let quality = self
                .quality
                .unwrap_or_else(|| panic!("{ITEM}:2280: attempt to perform arithmetic on field 'quality' (a nil value)"));
            let a = calc_local_slot(&mut mod_list, "Speed", ModType::Inc, mod_flag("Attack"));
            let b = calc_local_slot(&mut mod_list, "AlternateQualityLocalAttackSpeedPer8Quality", ModType::Inc, 0);
            let attack_speed_inc = a + (quality / 8.0 * b).floor();
            wd.set_num("AttackSpeedInc", attack_speed_inc);
            let attack_rate = round(weapon.AttackRateBase * (1.0 + attack_speed_inc / 100.0), Some(2.0));
            wd.set_num("AttackRate", attack_rate);
            let r1 = calc_local_slot(&mut mod_list, "WeaponRange", ModType::Base, 0);
            let r2 = calc_local_slot(&mut mod_list, "WeaponRangeMetre", ModType::Base, 0);
            let r3 = calc_local_slot(&mut mod_list, "AlternateQualityLocalWeaponRangePer10Quality", ModType::Base, 0);
            let range_bonus = r1 + 10.0 * r2 + (quality / 10.0 * r3).floor();
            wd.set_num("rangeBonus", range_bonus);
            wd.set_num("range", weapon.Range + range_bonus);
            let local_inc_ele = calc_local_slot(&mut mod_list, "LocalElementalDamage", ModType::Inc, 0);
            for dmg in DMG_TYPE_LIST {
                let (bmin, bmax) = if dmg == "Physical" {
                    (weapon.PhysicalMin, weapon.PhysicalMax)
                } else {
                    (None, None)
                };
                let mut min = bmin.unwrap_or(0.0) + calc_local_slot(&mut mod_list, &format!("{dmg}Min"), ModType::Base, 0);
                let mut max = bmax.unwrap_or(0.0) + calc_local_slot(&mut mod_list, &format!("{dmg}Max"), ModType::Base, 0);
                if dmg == "Physical" {
                    let phys_inc = calc_local_slot(&mut mod_list, "PhysicalDamage", ModType::Inc, 0);
                    let mut quality_scalar = quality;
                    if calc_local_slot(&mut mod_list, "AlternateQualityWeapon", ModType::Base, 0) > 0.0 {
                        quality_scalar = 0.0;
                    }
                    min = round(min * (1.0 + phys_inc / 100.0) * (1.0 + quality_scalar / 100.0), None);
                    max = round(max * (1.0 + phys_inc / 100.0) * (1.0 + quality_scalar / 100.0), None);
                } else if dmg != "Chaos" {
                    let local_inc = calc_local_slot(&mut mod_list, &format!("Local{dmg}Damage"), ModType::Inc, 0) + local_inc_ele;
                    min = round(min * (1.0 + local_inc / 100.0), None);
                    max = round(max * (1.0 + local_inc / 100.0), None);
                }
                if min > 0.0 && max > 0.0 {
                    wd.set_num(&format!("{dmg}Min"), min);
                    wd.set_num(&format!("{dmg}Max"), max);
                    let dps = (min + max) / 2.0 * attack_rate;
                    wd.set_num(&format!("{dmg}DPS"), dps);
                    if dmg != "Physical" && dmg != "Chaos" {
                        let e = wd.num("ElementalDPS").unwrap_or(0.0);
                        wd.set_num("ElementalDPS", e + dps);
                    }
                }
            }
            let c1 = calc_local_slot(&mut mod_list, "CritChance", ModType::Base, 0);
            let c2 = calc_local_slot(&mut mod_list, "CritChance", ModType::Inc, 0);
            let c3 = calc_local_slot(&mut mod_list, "AlternateQualityLocalCritChancePer4Quality", ModType::Inc, 0);
            wd.set_num(
                "CritChance",
                round(
                    (weapon.CritChanceBase + c1) * (1.0 + (c2 + (quality / 4.0 * c3).floor()) / 100.0),
                    Some(2.0),
                ),
            );
            for v in list_values_slot(&mod_list, "WeaponData") {
                set_key_value(&mut wd, &v);
            }
            let (attack, spell) = (mod_flag("Attack"), mod_flag("Spell"));
            let kw_attack = keyword_flag("Attack");
            let var = if slot_num == Some(1.0) { "MainHandAttack" } else { "OffHandAttack" };
            for sm in &mut mod_list {
                let m = &mut sm.m;
                let name = &*m.name;
                let cond = || tag(&[("type", Value::str("Condition")), ("var", Value::str(var))]);
                if ((name == "Accuracy" && m.flags == 0)
                    || (name == "ImpaleChance" && m.flags != spell)
                    || ((name == "LifeOnHit" || name == "ManaOnHit") && m.flags == attack)
                    || ((name == "PhysicalDamageLifeLeech" || name == "PhysicalDamageManaLeech") && m.flags == attack))
                    && (m.keyword_flags == 0 || m.keyword_flags == kw_attack)
                    && m.tags.is_empty()
                {
                    m.tags.push(cond()); // `mod[1] = { ... }` on the copy (MINEFIELD-RUST R4)
                } else if (name == "PoisonChance" || name == "BleedChance")
                    && m.flags != spell
                    && (m.tags.is_empty()
                        || (m.tags[0].ty().is_str("Condition") && m.tags[0].get("var").is_str("CriticalStrike") && m.tags.len() == 1))
                {
                    m.tags.push(cond());
                }
            }
            let mut total = 0.0;
            for dmg in DMG_TYPE_LIST {
                total += wd.num(&format!("{dmg}DPS")).unwrap_or(0.0);
            }
            wd.set_num("TotalDPS", total);
            self.weapon_data.get_or_insert_default().insert(NumKey(slot), wd);
        } else if let Some(armour) = &base.armour {
            let mut cl = |name: &str, ty: ModType| calc_local_slot(&mut mod_list, name, ty, 0);
            let armour_base = cl("Armour", ModType::Base) + armour.ArmourBaseMin.unwrap_or(0.0);
            let armour_variance = armour.ArmourBaseMax.unwrap_or(0.0) - armour.ArmourBaseMin.unwrap_or(0.0);
            let armour_evasion_base = cl("ArmourAndEvasion", ModType::Base);
            let evasion_base = cl("Evasion", ModType::Base) + armour.EvasionBaseMin.unwrap_or(0.0);
            let evasion_variance = armour.EvasionBaseMax.unwrap_or(0.0) - armour.EvasionBaseMin.unwrap_or(0.0);
            let evasion_es_base = cl("EvasionAndEnergyShield", ModType::Base);
            let es_base = cl("EnergyShield", ModType::Base) + armour.EnergyShieldBaseMin.unwrap_or(0.0);
            let es_variance = armour.EnergyShieldBaseMax.unwrap_or(0.0) - armour.EnergyShieldBaseMin.unwrap_or(0.0);
            let armour_es_base = cl("ArmourAndEnergyShield", ModType::Base);
            let ward_base = cl("Ward", ModType::Base) + armour.WardBaseMin.unwrap_or(0.0);
            let ward_variance = armour.WardBaseMax.unwrap_or(0.0) - armour.WardBaseMin.unwrap_or(0.0);
            let armour_inc = cl("Armour", ModType::Inc);
            let armour_evasion_inc = cl("ArmourAndEvasion", ModType::Inc);
            let evasion_inc = cl("Evasion", ModType::Inc);
            let evasion_es_inc = cl("EvasionAndEnergyShield", ModType::Inc);
            let es_inc = cl("EnergyShield", ModType::Inc);
            let ward_inc = cl("Ward", ModType::Inc);
            let armour_es_inc = cl("ArmourAndEnergyShield", ModType::Inc);
            let defences_inc = cl("Defences", ModType::Inc);
            let mut quality_scalar = self.quality.unwrap_or_else(|| panic!("{ITEM}:2364: quality is nil"));
            if cl("AlternateQualityArmour", ModType::Base) > 0.0 {
                quality_scalar = 0.0;
            }
            let ad = self.armour_data.get_or_insert_default();
            let pct = |value: f64, inc: f64, flat: f64, variance: f64| -> f64 {
                let p = ((value / ((1.0 + inc / 100.0) * (1.0 + (quality_scalar / 100.0)))) - flat) / variance;
                round(pob_lua::number::max2(pob_lua::number::min2(p, 1.0), 0.0), Some(4.0))
            };
            let a_inc = armour_inc + armour_evasion_inc + armour_es_inc + defences_inc;
            let e_inc = evasion_inc + armour_evasion_inc + evasion_es_inc + defences_inc;
            let s_inc = es_inc + armour_es_inc + evasion_es_inc + defences_inc;
            let w_inc = ward_inc + defences_inc;
            for (key, pkey, inc, flat, variance) in [
                ("Armour", "ArmourBasePercentile", a_inc, armour_base, armour_variance),
                ("Evasion", "EvasionBasePercentile", e_inc, evasion_base, evasion_variance),
                ("EnergyShield", "EnergyShieldBasePercentile", s_inc, es_base, es_variance),
                ("Ward", "WardBasePercentile", w_inc, ward_base, ward_variance),
            ] {
                if let Some(v) = ad.num(key)
                    && v > 0.0
                    && ad.get(pkey).is_nil()
                {
                    ad.set_num(pkey, pct(v, inc, flat, variance));
                }
            }
            let pc = |ad: &DataTable, k: &str| ad.num(k).unwrap_or(1.0);
            let qs = 1.0 + (quality_scalar / 100.0);
            let v = round(
                armour.ArmourBaseMin.unwrap_or(0.0) + armour_variance * pc(ad, "ArmourBasePercentile"),
                None,
            );
            ad.set_num("ArmourBase", v);
            let v = round(
                (armour_base + armour_evasion_base + armour_es_base + armour_variance * pc(ad, "ArmourBasePercentile"))
                    * (1.0 + a_inc / 100.0)
                    * qs,
                None,
            );
            ad.set_num("Armour", v);
            let v = round(
                armour.EvasionBaseMin.unwrap_or(0.0) + evasion_variance * pc(ad, "EvasionBasePercentile"),
                None,
            );
            ad.set_num("EvasionBase", v);
            let v = round(
                (evasion_base + armour_evasion_base + evasion_es_base + evasion_variance * pc(ad, "EvasionBasePercentile"))
                    * (1.0 + e_inc / 100.0)
                    * qs,
                None,
            );
            ad.set_num("Evasion", v);
            let v = round(
                armour.EnergyShieldBaseMin.unwrap_or(0.0) + es_variance * pc(ad, "EnergyShieldBasePercentile"),
                None,
            );
            ad.set_num("EnergyShieldBase", v);
            let v = round(
                (es_base + evasion_es_base + armour_es_base + es_variance * pc(ad, "EnergyShieldBasePercentile"))
                    * (1.0 + s_inc / 100.0)
                    * qs,
                None,
            );
            ad.set_num("EnergyShield", v);
            let v = round(
                armour.WardBaseMin.unwrap_or(0.0) + ward_variance * pc(ad, "WardBasePercentile"),
                None,
            );
            ad.set_num("WardBase", v);
            let v = round(
                (ward_base + ward_variance * pc(ad, "WardBasePercentile")) * (1.0 + w_inc / 100.0) * qs,
                None,
            );
            ad.set_num("Ward", v);
            for (key, pkey) in [
                ("Armour", "ArmourBasePercentile"),
                ("Evasion", "EvasionBasePercentile"),
                ("EnergyShield", "EnergyShieldBasePercentile"),
                ("Ward", "WardBasePercentile"),
            ] {
                if ad.get(pkey).is_nil() && ad.num(key).is_some_and(|v| v > 0.0) {
                    ad.set_num(pkey, 1.0);
                }
            }
            if let Some(block) = armour.BlockChance {
                let b = calc_local_slot(&mut mod_list, "BlockChance", ModType::Base, 0);
                let i = calc_local_slot(&mut mod_list, "BlockChance", ModType::Inc, 0);
                self.armour_data
                    .get_or_insert_default()
                    .set_num("BlockChance", ((block + b) * (1.0 + i / 100.0)).floor());
            }
            if let Some(pen) = armour.MovementPenalty {
                let t = tag(&[
                    ("type", Value::str("Condition")),
                    ("var", Value::str("IgnoreMovementPenalties")),
                    ("neg", Value::Bool(true)),
                ]);
                new_mod(
                    &mut mod_list,
                    "MovementSpeed",
                    ModType::Inc,
                    Value::Num(-pen),
                    mod_source.as_deref(),
                    vec![t],
                );
            }
            let ad = self.armour_data.get_or_insert_default();
            for v in list_values_slot(&mod_list, "ArmourData") {
                set_key_value(ad, &v);
            }
        } else if let Some(flask) = &base.flask {
            let mut fd = self.flask_data.take().unwrap_or_default();
            let quality = || self.quality.unwrap_or_else(|| panic!("{ITEM}:2425: quality is nil"));
            let duration_inc = calc_local_slot(&mut mod_list, "Duration", ModType::Inc, 0);
            let duration_more = calc_local_slot(&mut mod_list, "Duration", ModType::More, 0);
            if flask.life.is_some() || flask.mana.is_some() {
                let instant_perc = calc_local_slot(&mut mod_list, "FlaskInstantRecovery", ModType::Base, 0);
                fd.set_num("instantPerc", instant_perc);
                fd.set_num(
                    "instantLowLifePerc",
                    calc_local_slot(&mut mod_list, "FlaskLowLifeInstantRecovery", ModType::Base, 0),
                );
                let recovery_mod = 1.0 + calc_local_slot(&mut mod_list, "FlaskRecovery", ModType::Inc, 0) / 100.0;
                let rate_mod = 1.0 + calc_local_slot(&mut mod_list, "FlaskRecoveryRate", ModType::Inc, 0) / 100.0;
                fd.set_num(
                    "duration",
                    round(flask.duration * (1.0 + duration_inc / 100.0) / rate_mod * duration_more, Some(1.0)),
                );
                if let Some(life) = flask.life {
                    let lb = life * (1.0 + quality() / 100.0) * recovery_mod;
                    fd.set_num("lifeBase", lb);
                    let li = lb * instant_perc / 100.0;
                    let lg = lb * (1.0 - instant_perc / 100.0);
                    fd.set_num("lifeInstant", li);
                    fd.set_num("lifeGradual", lg);
                    fd.set_num("lifeTotal", li + lg);
                    fd.set_num(
                        "lifeAdditional",
                        calc_local_slot(&mut mod_list, "FlaskAdditionalLifeRecovery", ModType::Base, 0),
                    );
                    // `calcLocal(baseList, ...)` -- the BASE list (upstream), which removes from `baseModList`.
                    fd.set(
                        "lifeEffectNotRemoved",
                        calc_local_flag_value(base_list, "LifeFlaskEffectNotRemoved"),
                    );
                }
                if let Some(mana) = flask.mana {
                    let mb = mana * (1.0 + quality() / 100.0) * recovery_mod;
                    fd.set_num("manaBase", mb);
                    let mi = mb * instant_perc / 100.0;
                    let mg = mb * (1.0 - instant_perc / 100.0);
                    fd.set_num("manaInstant", mi);
                    fd.set_num("manaGradual", mg);
                    fd.set_num("manaTotal", mi + mg);
                    fd.set(
                        "manaEffectNotRemoved",
                        calc_local_flag_value(base_list, "ManaFlaskEffectNotRemoved"),
                    );
                }
            } else {
                fd.set_num(
                    "duration",
                    round(
                        flask.duration * (1.0 + duration_inc / 100.0) * (1.0 + quality() / 100.0) * duration_more,
                        Some(1.0),
                    ),
                );
            }
            fd.set_num(
                "chargesMax",
                flask.charges_max + calc_local_slot(&mut mod_list, "FlaskCharges", ModType::Base, 0),
            );
            let cu = calc_local_slot(&mut mod_list, "FlaskChargesUsed", ModType::Inc, 0);
            fd.set_num("chargesUsed", (flask.charges_used * (1.0 + cu / 100.0)).floor());
            fd.set_num(
                "gainMod",
                1.0 + calc_local_slot(&mut mod_list, "FlaskChargeRecovery", ModType::Inc, 0) / 100.0,
            );
            let e1 = calc_local_slot(&mut mod_list, "FlaskEffect", ModType::Inc, 0);
            let e2 = calc_local_slot(&mut mod_list, "LocalEffect", ModType::Inc, 0);
            fd.set_num("effectInc", e1 + e2);
            for v in list_values_slot(&mod_list, "FlaskData") {
                set_key_value(&mut fd, &v);
            }
            self.flask_data = Some(fd);
        } else if let Some(t) = &base.tincture {
            let mut td = self.tincture_data.take().unwrap_or_default();
            let a = calc_local_slot(&mut mod_list, "TinctureManaBurnRate", ModType::Inc, 0);
            let b = calc_local_slot(&mut mod_list, "TinctureManaBurnRate", ModType::More, 0);
            td.set_num("manaBurn", (t.mana_burn + 0.01) / (1.0 + a / 100.0) / (1.0 + b / 100.0));
            let c1 = calc_local_slot(&mut mod_list, "TinctureCooldownRecovery", ModType::Inc, 0);
            let c2 = calc_local_slot(&mut mod_list, "CooldownRecovery", ModType::Inc, 0);
            td.set_num("cooldownInc", c1 + c2);
            td.set_num("cooldown", t.cooldown / (1.0 + (c1 + c2) / 100.0));
            let e1 = calc_local_slot(&mut mod_list, "TinctureEffect", ModType::Inc, 0);
            let e2 = calc_local_slot(&mut mod_list, "LocalEffect", ModType::Inc, 0);
            td.set_num("effectInc", e1 + e2);
            for v in list_values_slot(&mod_list, "TinctureData") {
                set_key_value(&mut td, &v);
            }
            self.tincture_data = Some(td);
        } else if self.ty.as_deref() == Some("Jewel") {
            self.build_jewel_data(&mut mod_list);
        }
        mod_list
    }

    /// Item.lua:2465-2515 -- the jewel branch of BuildModListForSlotNum.
    fn build_jewel_data(&mut self, mod_list: &mut Vec<SlotMod>) {
        if find_plain(&self.name, "Grand Spectrum").is_some() {
            let spectrum = Rc::new(create_mod(
                "Multiplier:GrandSpectrum",
                Some(ModType::Base),
                Value::Num(1.0),
                Some(&self.name),
                0,
                0,
                vec![],
            ));
            mod_list.push(SlotMod {
                m: (*spectrum).clone(),
                source_slot: None,
            });
            let mut v = Table::default();
            v.hash.insert(Rc::from("mod"), Value::Mod(spectrum));
            mod_list.push(SlotMod {
                m: create_mod(
                    "MinionModifier",
                    Some(ModType::List),
                    Value::table(v),
                    Some(&self.name),
                    0,
                    0,
                    vec![],
                ),
                source_slot: None,
            });
        }
        let mut jd = self.jewel_data.take().unwrap_or_default();
        for func in list_values_slot(mod_list, "JewelFunc") {
            let mut fl = match jd.get("funcList") {
                Value::Table(t) => (*t).clone(),
                _ => Table::default(),
            };
            fl.array.push(func);
            jd.set("funcList", Value::table(fl));
        }
        for v in list_values_slot(mod_list, "JewelData") {
            set_key_value(&mut jd, &v);
        }
        // L4: `if modList:List(...) then` -- List always returns a table, and `{}` is truthy.
        let mut esc = Table::default();
        for v in list_values_slot(mod_list, "ImpossibleEscapeKeystones") {
            let k = v.get("key");
            let val = v.get("value");
            match &k {
                Value::Str(s) => {
                    if val.is_nil() {
                        esc.hash.shift_remove(s);
                    } else {
                        esc.hash.insert(s.clone(), val);
                    }
                }
                _ => unimplemented!("{ITEM}:2485 impossibleEscapeKeystones with a non-string key"),
            }
        }
        jd.set("impossibleEscapeKeystones", Value::table(esc));
        if let Some(cj) = self.cluster_jewel {
            let notables = Table {
                array: list_values_slot(mod_list, "ClusterJewelNotable"),
                ..Table::default()
            };
            jd.set("clusterJewelNotables", Value::table(notables));
            let added = Table {
                array: list_values_slot(mod_list, "AddToClusterJewelNode"),
                ..Table::default()
            };
            jd.set("clusterJewelAddedMods", Value::table(added));
            if cj.size == "Small" && jd.get("clusterJewelSkill").is_str("affliction_curse_effect") {
                jd.set("clusterJewelSkill", Value::str("affliction_curse_effect_small"));
            } else if cj.size == "Medium" && jd.get("clusterJewelSkill").is_str("affliction_curse_effect_small") {
                jd.set("clusterJewelSkill", Value::str("affliction_curse_effect"));
            }
            if jd.get("clusterJewelNodeCount").truthy() {
                let n = jd
                    .num("clusterJewelNodeCount")
                    .unwrap_or_else(|| panic!("{ITEM}:2502: clusterJewelNodeCount is not a number"));
                jd.set_num(
                    "clusterJewelNodeCount",
                    pob_lua::number::min2(pob_lua::number::max2(n, cj.min_nodes), cj.max_nodes),
                );
            }
            let skill = jd.get("clusterJewelSkill");
            if skill.truthy() && !skill.as_str().is_some_and(|s| cj.skills.contains_key(s)) {
                jd.set("clusterJewelSkill", Value::Nil);
            }
            if self.cluster_jewel_skill.is_none() {
                self.cluster_jewel_skill = jd.get("clusterJewelSkill").as_str().map(str::to_owned);
            }
            if self.cluster_jewel_node_count.is_none() {
                self.cluster_jewel_node_count = jd.num("clusterJewelNodeCount");
            }
            // `keystone or ((skill or smalls) and nodeCount) or (socketOverride and nothingness)`
            let or = |a: Value, b: Value| if a.truthy() { a } else { b };
            let and = |a: Value, b: Value| if a.truthy() { b } else { a };
            let valid = or(
                or(
                    jd.get("clusterJewelKeystone"),
                    and(
                        or(jd.get("clusterJewelSkill"), jd.get("clusterJewelSmallsAreNothingness")),
                        jd.get("clusterJewelNodeCount"),
                    ),
                ),
                and(jd.get("clusterJewelSocketCountOverride"), jd.get("clusterJewelNothingnessCount")),
            );
            jd.set("clusterJewelValid", valid);
        }
        self.jewel_data = Some(jd);
    }

    /// Every mod line list in BuildModList order with the list name (for dumps).
    /// `self.rangeLineList` (Item.lua:2568-2597): the positions (list id as in `mod_line_by_id`, index) of the lines with a slider or a mutation.
    pub fn range_line_list(&self) -> Vec<(u8, usize)> {
        let mut out = Vec::new();
        for (lid, list) in [
            (1u8, &self.enchant_mod_lines),
            (2, &self.scourge_mod_lines),
            (3, &self.implicit_mod_lines),
            (4, &self.explicit_mod_lines),
            (5, &self.crucible_mod_lines),
        ] {
            for (i, ml) in list.iter().enumerate() {
                if ml.show_slider == Some(true)
                    || (ml.mod_id.is_some() && ml.new_mod_id.is_some() && self.get_mod_line_variant_count(ml) > 0)
                {
                    out.push((lid, i));
                }
            }
        }
        out
    }

    /// The lists by the ids of `range_line_list` and of a tooltip line: 1 enchant, 2 scourge, 3 implicit, 4 explicit, 5 crucible.
    pub fn mod_line_by_id(&mut self, id: (u8, usize)) -> Option<&mut ModLine> {
        match id.0 {
            1 => self.enchant_mod_lines.get_mut(id.1),
            2 => self.scourge_mod_lines.get_mut(id.1),
            3 => self.implicit_mod_lines.get_mut(id.1),
            4 => self.explicit_mod_lines.get_mut(id.1),
            5 => self.crucible_mod_lines.get_mut(id.1),
            _ => None,
        }
    }

    /// `#self.modMagnitudeMods == 0`.
    pub fn mod_magnitude_empty(&self) -> bool {
        self.mod_magnitude_mods.is_empty()
    }

    /// `self[influence.key] = v`.
    pub fn set_influence(&mut self, key: &str, v: bool) {
        if let Some(i) = INFLUENCE_INFO.iter().position(|(k, _)| *k == key) {
            self.influence[i] = v;
        }
    }

    /// `new("Item"):Item()` + `item.name`, `item.base`, `item.baseName` (ItemsTab.lua:2682-2685).
    pub fn new_crafted(name: &str, base: &'static ItemBase) -> Item {
        Item {
            parse_mode: pob_parser::ParseMode::Full,
            name: name.to_owned(),
            base: Some(base),
            base_name: Some(name.to_owned()),
            ..Item::default()
        }
    }

    /// `self.rareLikeUnique.ignoreModType`.
    pub fn ignore_mod_type(&self) -> bool {
        self.rare_like_unique.is_some_and(|r| r.ignore_mod_type)
    }

    /// `ItemClass:ResetInfluence()` (Item.lua:102-106).
    pub fn reset_influence(&mut self) {
        self.influence = [false; 8];
    }

    pub fn mod_line_lists(&self) -> [(&'static str, &Vec<ModLine>); 7] {
        [
            ("enchantModLines", &self.enchant_mod_lines),
            ("scourgeModLines", &self.scourge_mod_lines),
            ("classRequirementModLines", &self.class_requirement_mod_lines),
            ("implicitModLines", &self.implicit_mod_lines),
            ("explicitModLines", &self.explicit_mod_lines),
            ("crucibleModLines", &self.crucible_mod_lines),
            ("buffModLines", &self.buff_mod_lines),
        ]
    }
}

/// The strings of a list value (`jewelData.clusterJewelNotables`).
pub fn value_strings(v: &Value) -> Vec<String> {
    v.ipairs().iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()
}

/// `weaponData[value.key] = value.value` for one `{ key = ..., value = ... }` LIST value.
fn set_key_value(t: &mut DataTable, v: &Value) {
    match v.get("key") {
        Value::Str(k) => t.set(&k, v.get("value")),
        other => unimplemented!("{ITEM}: a *Data LIST value keyed by a {}", other.type_name()),
    }
}

/// Item.lua:2186-2220 `calcLocal` for BASE/INC/MORE: sums (and REMOVES) the local mods -- exact name/type/flags, no keyword
/// flags, no tag except a leading `InSlot`.
fn calc_local_in<T>(list: &mut Vec<T>, get: impl Fn(&T) -> &Mod, name: &str, ty: ModType, flags: u32) -> (f64, Option<Value>) {
    let mut num = if ty == ModType::More { 1.0 } else { 0.0 };
    let mut flag: Option<Value> = None;
    let mut i = 0;
    while i < list.len() {
        let m = get(&list[i]);
        if &*m.name == name
            && m.ty == Some(ty)
            && m.flags == flags
            && m.keyword_flags == 0
            && (m.tags.is_empty() || m.tags[0].ty().is_str("InSlot"))
        {
            match ty {
                // `result = result or mod.value`
                ModType::Flag => {
                    if !flag.as_ref().is_some_and(Value::truthy) {
                        flag = Some(m.value.clone());
                    }
                }
                ModType::More => {
                    let v = m
                        .value
                        .as_num()
                        .unwrap_or_else(|| panic!("{ITEM}:2207: attempt to perform arithmetic on field 'value'"));
                    num *= (100.0 + v) / 100.0;
                }
                _ => {
                    let v = m
                        .value
                        .as_num()
                        .unwrap_or_else(|| panic!("{ITEM}:2209: attempt to perform arithmetic on field 'value'"));
                    num += v;
                }
            }
            list.remove(i);
        } else {
            i += 1;
        }
    }
    (num, flag)
}

fn calc_local(list: &mut Vec<Mod>, name: &str, ty: ModType, flags: u32) -> f64 {
    calc_local_in(list, |m| m, name, ty, flags).0
}

fn calc_local_slot(list: &mut Vec<SlotMod>, name: &str, ty: ModType, flags: u32) -> f64 {
    calc_local_in(list, |m| &m.m, name, ty, flags).0
}

/// `calcLocal(list, name, "FLAG", flags)` as a Lua truth value (`false` or the first truthy `mod.value`).
fn calc_local_flag(list: &mut Vec<Mod>, name: &str, flags: u32) -> bool {
    calc_local_in(list, |m| m, name, ModType::Flag, flags).1.is_some_and(|v| v.truthy())
}

/// `calcLocal(list, name, "FLAG", 0)` as the stored value: `false` when nothing matched, else the first truthy `mod.value`.
fn calc_local_flag_value(list: &mut Vec<Mod>, name: &str) -> Value {
    match calc_local_in(list, |m| m, name, ModType::Flag, 0).1 {
        Some(v) if v.truthy() => v,
        _ => Value::Bool(false),
    }
}

fn store_of<'a>(mods: impl Iterator<Item = &'a Mod>, name: &str) -> pob_mods::StoreRef {
    let store = ModStore::new_list(None);
    {
        let mut s = store.borrow_mut();
        for m in mods.filter(|m| &*m.name == name) {
            s.add_mod(Rc::new(m.clone()));
        }
    }
    store
}

/// `modList:List(nil, name)` (ModStore.lua:215, EvalMod with a nil cfg) over a plain mod array.
pub fn list_values(mods: &[Mod], name: &str) -> Vec<Value> {
    let store = store_of(mods.iter(), name);
    store.borrow().list(None, &[name]).unwrap_or_else(|e| panic!("{e}"))
}

fn list_values_slot(mods: &[SlotMod], name: &str) -> Vec<Value> {
    let store = store_of(mods.iter().map(|m| &m.m), name);
    store.borrow().list(None, &[name]).unwrap_or_else(|e| panic!("{e}"))
}

/// Item.lua:1130-1170 -- the advanced-copy `value(min-max)` endpoints and enum ranges (`%b()`); a no-op on PoB-XML text
/// (no `value(` form survives the `%1` gsub before it).
fn resolve_advanced_copy_ranges(line: &str, ml: &mut ModLine) -> String {
    let mut best_delta = -1.0;
    let mut best_range = -1.0;
    let mut first_roll: Option<f64> = None;
    let mut independent = false;
    let mut line = gsub_fn(line, "(%s*)(%b())", None, |c| {
        let (space, range) = (&c[0], &c[1]);
        if find_plain(range, "-").is_some() && !matches(range, "%d") {
            Some(String::new())
        } else {
            Some(format!("{space}{range}"))
        }
    });
    let advanced_copy_line = line.clone();
    for c in gmatch(&advanced_copy_line, "(%-?%d+%.?%d*)%((%-?%d+%.?%d*%-%-?%d+%.?%d*)%)") {
        let (value, range) = (&c[0], &c[1]);
        let mm = match_(range, "(%-?%d+%.?%d*)%-(%-?%d+%.?%d*)", ITEM).expect("range");
        let (mut min, mut max) = (mm[0].clone(), mm[1].clone());
        if tonumber(&min) > tonumber(&max) {
            std::mem::swap(&mut min, &mut max);
        }
        let (vmin, vmax, v) = (tonumber(&min).expect("n"), tonumber(&max).expect("n"), tonumber(value).expect("n"));
        let delta = vmax - vmin;
        let roll = if delta > 0.0 { round((v - vmin) / delta, Some(6.0)) } else { 0.5 };
        if first_roll.is_some_and(|f| f != roll) {
            independent = true;
        }
        first_roll.get_or_insert(roll);
        if delta > best_delta {
            best_range = roll;
            best_delta = delta;
        }
        let pat = format!("{value}%({}%)", gsub(range, "%-", "%%-", None));
        let repl = if !(0.0..=1.0).contains(&best_range) {
            value.clone()
        } else {
            format!("{}({min}-{max})", if v < 0.0 { "+" } else { "" })
        };
        line = gsub(&line, &pat, &repl, None);
    }
    if independent {
        line = gsub(&advanced_copy_line, "(%-?%d+%.?%d*)%(%-?%d+%.?%d*%-%-?%d+%.?%d*%)", "%1", None);
    } else if (0.0..=1.0).contains(&best_range) {
        ml.range = Some(best_range);
        // Item.lua:1188 (3dbca4463 #10374)
        ml.range_from_value = true;
    }
    line
}

impl pob_mods::ItemLike for Item {
    fn name(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn item_type(&self) -> Option<&str> {
        self.ty.as_deref()
    }
    fn rarity(&self) -> Option<&str> {
        Some(&self.rarity)
    }
    fn corrupted(&self) -> Option<bool> {
        self.corrupted.then_some(true)
    }
    fn shaper(&self) -> Option<bool> {
        self.influence[0].then_some(true)
    }
    fn elder(&self) -> Option<bool> {
        self.influence[1].then_some(true)
    }
    fn find_modifier_substring(&self, substring: &str, slot_name: &str) -> bool {
        self.find_modifier_substring_impl(substring, slot_name)
    }
}
