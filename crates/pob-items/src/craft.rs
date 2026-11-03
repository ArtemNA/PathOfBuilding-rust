//! The editing half of `Classes/Item.lua`: `Craft` (:2070-2133), `MutateMod` (:1661-1696), `GetModSpawnWeight` (:1708-1767), `GetNecropolisModSpawnWeight` (:1769-1780), `CheckIfModIsDelve` (:1782-1784), `CanHaveMod` (:2721-2748), and `self.affixes` as full mods. No result of a load changes: only `pob_ui::tabs::items` calls these.

use std::collections::{BTreeMap, HashSet};

use indexmap::IndexMap;

use crate::build_raw::ItemAffixes;
use crate::item::{Affix, AffixRange, Item, ModLine, sort_crafted_mod_lines};
use crate::lua::gsub_fn;

/// One `data.itemMods.<Category>[modId]`.
#[derive(Clone, Debug, Default)]
pub struct AffixMod {
    pub id: String,
    pub lines: Vec<String>,
    pub ty: Option<String>,
    pub affix: String,
    pub group: Option<String>,
    pub level: f64,
    pub stat_order: Vec<f64>,
    pub mod_tags: Vec<String>,
    pub tags: Vec<String>,
    pub weight_key: Vec<String>,
    pub weight_val: Vec<f64>,
    pub weight_multiplier_key: Vec<String>,
    pub weight_multiplier_val: Vec<f64>,
}

fn strs(v: &serde_json::Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect()
}

fn nums(v: &serde_json::Value, k: &str) -> Vec<f64> {
    v.get(k)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_f64)
        .collect()
}

impl AffixMod {
    /// The JSON the data converter wrote.
    pub fn from_json(id: &str, v: &serde_json::Value) -> AffixMod {
        let s = |k: &str| v.get(k).and_then(serde_json::Value::as_str).map(str::to_owned);
        AffixMod {
            id: id.to_owned(),
            lines: strs(v, "lines"),
            ty: s("type"),
            affix: s("affix").unwrap_or_default(),
            group: s("group"),
            level: v.get("level").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            stat_order: nums(v, "statOrder"),
            mod_tags: strs(v, "modTags"),
            tags: strs(v, "tags"),
            weight_key: strs(v, "weightKey"),
            weight_val: nums(v, "weightVal"),
            weight_multiplier_key: strs(v, "weightMultiplierKey"),
            weight_multiplier_val: nums(v, "weightMultiplierVal"),
        }
    }
}

impl ItemAffixes {
    /// `self.affixes` as full mods, in the table's order.
    pub fn mods(&'static self) -> &'static IndexMap<String, AffixMod> {
        self.full
            .get_or_init(|| self.values.iter().map(|(id, v)| (id.clone(), AffixMod::from_json(id, v))).collect())
    }
}

/// `data.casterTagCrucibleUniques` (Data.lua:1306-1324).
const CASTER_TAG_CRUCIBLE_UNIQUES: [&str; 17] = [
    "Atziri's Rule",
    "Cane of Kulemak",
    "Cane of Unravelling",
    "Cospri's Malice",
    "Cybil's Paw",
    "Disintegrator",
    "Duskdawn",
    "Geofri's Devotion",
    "Mjolner",
    "Pledge of Hands",
    "Soulwrest",
    "Taryn's Shiver",
    "The Rippling Thoughts",
    "The Surging Thoughts",
    "The Whispering Ice",
    "Tremor Rod",
    "Xirgil's Crank",
];

/// `data.minionTagCrucibleUniques` (Data.lua:1325-).
const MINION_TAG_CRUCIBLE_UNIQUES: &[&str] = &[
    "Arakaali's Fang",
    "Ashcaller",
    "Chaber Cairn",
    "Chober Chaber",
    "Clayshaper",
    "Earendel's Embrace",
    "Femurs of the Saints",
    "Jorrhast's Blacksteel",
    "Law of the Wilds",
    "Midnight Bargain",
    "Mon'tregul's Grasp",
    "Null's Inclination",
    "Queen's Decree",
    "Queen's Escape",
    "Replica Earendel's Embrace",
    "Replica Midnight Bargain",
    "Severed in Sleep",
    "Soulwrest",
    "The Black Cane",
    "The Iron Mass",
    "The Scourge",
    "United in Dream",
];

/// `base.tags[key]`.
fn base_has(tags: &pob_data::wire::OrderedMap<bool>, key: &str) -> bool {
    tags.get(key).copied().unwrap_or(false)
}

impl Item {
    /// `self.affixes`.
    pub fn affix_mods(&self) -> Option<&'static IndexMap<String, AffixMod>> {
        self.affixes.map(ItemAffixes::mods)
    }

    /// `ItemClass:CheckIfModIsDelve(mod)`.
    pub fn check_if_mod_is_delve(&self, m: &AffixMod) -> bool {
        m.affix == "Subterranean" || m.affix == "of the Underground"
    }

    /// `ItemClass:GetModSpawnWeight(mod, includeTags, excludeTags, baseTags)`.
    pub fn get_mod_spawn_weight(
        &self,
        m: &AffixMod,
        include: &HashSet<String>,
        exclude: &HashSet<String>,
        base_tags: Option<&pob_data::wire::OrderedMap<bool>>,
    ) -> f64 {
        let Some(own) = self.base else { return 0.0 };
        let tags = base_tags.unwrap_or(&own.tags);
        let influence = |key: &str| {
            own.influence_tags.as_ref().is_some_and(|t| {
                crate::item::INFLUENCE_INFO
                    .iter()
                    .any(|(k, _)| self.influence(k) && t.get(*k).is_some_and(|v| v == key))
            })
        };
        if self.restrict_tag {
            for key in &m.mod_tags {
                if (key == "caster" && self.no_caster) || (key == "attack" && self.no_attack) {
                    return 0.0;
                }
            }
        }
        if self.restrict_damage_type
            && let Some(only) = self.only_damage_type
        {
            for key in &m.mod_tags {
                if ["fire", "cold", "lightning", "chaos", "physical"].contains(&key.as_str()) && key != only {
                    return 0.0;
                }
            }
        }
        let mut weight = 0.0;
        for (i, key) in m.weight_key.iter().enumerate() {
            if (base_has(tags, key) || include.contains(key) || influence(key)) && !exclude.contains(key) {
                weight = if influence(key) && m.affix.contains("Elevated") {
                    1000.0
                } else {
                    m.weight_val[i]
                };
                break;
            }
        }
        for (i, key) in m.weight_multiplier_key.iter().enumerate() {
            if (base_has(tags, key) || include.contains(key) || influence(key)) && !exclude.contains(key) {
                weight = weight * m.weight_multiplier_val[i] / 100.0;
                break;
            }
        }
        weight
    }

    /// `ItemClass:GetNecropolisModSpawnWeight(mod)`.
    pub fn get_necropolis_mod_spawn_weight(&self, m: &AffixMod) -> f64 {
        let Some(base) = self.base else { return 0.0 };
        for (i, key) in m.weight_key.iter().enumerate() {
            if base_has(&base.tags, key.strip_prefix("necropolis_").unwrap_or(key)) {
                return m.weight_val[i];
            }
        }
        0.0
    }

    /// `ItemClass:CanHaveMod(mod, includeTags)`.
    pub fn can_have_mod(&self, m: &AffixMod, include: &HashSet<String>) -> bool {
        let none = HashSet::new();
        let mut include = include.clone();
        let title = self.title.as_deref().unwrap_or("");
        if CASTER_TAG_CRUCIBLE_UNIQUES.contains(&title) {
            include.insert("caster_unique_weapon".to_owned());
        }
        if MINION_TAG_CRUCIBLE_UNIQUES.contains(&title) {
            include.insert("minion_unique_weapon".to_owned());
        }
        let include = &include;
        if let Some(bases) = self.rare_like_unique.and_then(|r| r.valid_bases.as_ref()) {
            return bases
                .iter()
                .any(|tags| self.get_mod_spawn_weight(m, include, &none, Some(tags)) > 0.0);
        }
        if self.can_have_only_support_skills_crucible_tree {
            return m
                .weight_key
                .iter()
                .position(|k| k == "crucible_unique_staff")
                .is_some_and(|i| m.weight_val[i] != 0.0);
        }
        if self.can_have_shield_crucible_tree {
            let inc: HashSet<String> = ["crucible_unique_helmet", "shield"].iter().map(|s| (*s).to_owned()).collect();
            return self.get_mod_spawn_weight(m, &inc, &none, None) > 0.0;
        }
        if self.can_have_two_handed_sword_crucible_tree {
            let inc: HashSet<String> = std::iter::once("two_hand_weapon".to_owned()).collect();
            let exc: HashSet<String> = std::iter::once("one_hand_weapon".to_owned()).collect();
            return self.get_mod_spawn_weight(m, &inc, &exc, None) > 0.0;
        }
        self.get_mod_spawn_weight(m, include, &none, None) > 0.0
    }

    /// `ItemClass:Craft()`.
    pub fn craft(&mut self) {
        let saved: Vec<ModLine> = self
            .explicit_mod_lines
            .iter()
            .filter(|m| {
                m.flag("crafted") || m.flag("custom") || (self.rare_like_unique.is_some() && !(m.flag("prefix") || m.flag("suffix")))
            })
            .cloned()
            .collect();
        self.explicit_mod_lines.clear();
        self.name_prefix.clear();
        self.name_suffix.clear();
        let base = self.base.expect("Craft: a base");
        self.requirements.level = base.req.level;
        let half = self.affix_limit / 2.0;
        let mods = self.affix_mods();
        let mut by_order: BTreeMap<u64, usize> = BTreeMap::new();
        let limits = [self.prefixes_limit.unwrap_or(half), self.suffixes_limit.unwrap_or(half)];
        for (which, limit) in limits.iter().enumerate() {
            for i in 0..*limit as usize {
                let list = if which == 0 { &mut self.prefixes } else { &mut self.suffixes };
                if list.len() <= i {
                    list.push(Affix {
                        mod_id: "None".to_owned(),
                        range: None,
                        fractured: false,
                    });
                }
                let affix = list[i].clone();
                let Some(m) = mods.and_then(|t| t.get(&affix.mod_id)) else {
                    continue;
                };
                match m.ty.as_deref() {
                    Some("Prefix") => self.name_prefix = format!("{} {}", m.affix, self.name_prefix),
                    Some("Suffix") => self.name_suffix = format!("{} {}", self.name_suffix, m.affix),
                    _ => {}
                }
                self.requirements.level = Some(self.requirements.level.unwrap_or(0.0).max((m.level * 0.8).floor()));
                let range = match &affix.range {
                    Some(AffixRange::Num(n)) => *n,
                    Some(AffixRange::List(l)) => l.first().copied().unwrap_or(0.5),
                    None => 0.5,
                };
                for (li, line) in m.lines.iter().enumerate() {
                    let line = crate::apply_range::apply_range(line, range, None, None, self.parse_mode);
                    let order = m.stat_order[li];
                    if let Some(&at) = by_order.get(&order.to_bits()) {
                        // :2097-2103 the numbers of the two lines are added
                        let others: Vec<String> = crate::lua::gmatch(&line, "(%d+)").into_iter().map(|c| c[0].clone()).collect();
                        let mut k = 0;
                        let merged = gsub_fn(&self.explicit_mod_lines[at].line, "%d+", None, |c| {
                            let a = crate::lua::tonumber(&c[0]).unwrap_or(0.0);
                            let b = others.get(k).and_then(|x| crate::lua::tonumber(x)).unwrap_or(0.0);
                            k += 1;
                            Some(crate::lua::tostring(a + b))
                        });
                        self.explicit_mod_lines[at].line = merged;
                    } else {
                        let mut ml = ModLine {
                            line,
                            order: Some(order),
                            mod_tags: m.mod_tags.clone(),
                            ..ModLine::default()
                        };
                        if affix.fractured {
                            ml.set_flag_pub("fractured");
                        }
                        match m.ty.as_deref() {
                            Some("Prefix") => ml.set_flag_pub("prefix"),
                            Some("Suffix") => ml.set_flag_pub("suffix"),
                            _ => {}
                        }
                        self.explicit_mod_lines.push(ml);
                        by_order.insert(order.to_bits(), self.explicit_mod_lines.len() - 1);
                    }
                }
            }
        }
        self.explicit_mod_lines.extend(saved);
        if self.explicit_mod_lines.len() > 1 {
            sort_crafted_mod_lines(&mut self.explicit_mod_lines);
        }
        self.build_and_parse_raw();
    }

    /// `ItemClass:MutateMod(modId, newModId, mutatedValue)`.
    pub fn mutate_mod(&mut self, mod_id: &str, new_mod_id: &str, mutated: bool) {
        let table = if mutated {
            pob_data::item_mods::foulborn()
        } else {
            pob_data::item_mods::item_exclusive()
        };
        let Some(new_mod) = table.get(new_mod_id) else {
            return;
        };
        let mut insert_idx = None;
        let mut carried = (None, None, None);
        let mut i = 0;
        while i < self.explicit_mod_lines.len() {
            let ml = &self.explicit_mod_lines[i];
            if ml.mod_id.as_deref() == Some(mod_id) && self.check_mod_line_variant(ml) {
                if insert_idx.is_none() {
                    insert_idx = Some(i);
                    carried = (ml.variant_list.clone(), ml.version_list.clone(), ml.variant_group_list.clone());
                }
                self.explicit_mod_lines.remove(i);
            } else {
                i += 1;
            }
        }
        let mut ml = ModLine {
            line: new_mod.lines.join("\n"),
            mod_tags: crate::build_raw::mod_file(if mutated {
                "item-mods/Foulborn.json"
            } else {
                "item-mods/ItemExclusive.json"
            })
            .get(new_mod_id)
            .map_or_else(Vec::new, |v| strs(v, "modTags")),
            variant_list: carried.0,
            version_list: carried.1,
            variant_group_list: carried.2,
            mod_group: Some(new_mod_id.to_owned()),
            ..ModLine::default()
        };
        if mutated {
            ml.set_flag_pub("mutated");
        }
        self.explicit_mod_lines
            .insert(insert_idx.expect("Item.lua:1683: table.insert at nil"), ml);
        self.build_and_parse_raw();
    }
}
