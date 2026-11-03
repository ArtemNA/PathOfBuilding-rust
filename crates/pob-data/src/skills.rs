//! `data.skills` (Data/Skills/*.lua via `data/skills.json`, 1 526 granted effects) with the post-processing of Modules/Data.lua:1049-1080
//! and the LAZY `statMap` of :1033-1045.
//!
//! * The converter snapshotted the table right after Data.lua (MINEFIELD D2): `id`, `modSource`, `processMod` on `baseMods`/`levelMods`/
//!   the own `statMap` entries (`source`, `ActorCondition` for `notMinionStat`, `hasGlobalEffect`) are already in the data -- the loader does
//!   not repeat them.
//! * `skillStatMapMeta.__index` (:1033-1045): reading a stat the effect's own `statMap` lacks copies `data.skillStatMap[stat]`
//!   (`copyTable` -- R4: every effect gets its own mods), runs `processMod(t._grantedEffect, mod, stat)` on each mod and stores the copy in
//!   the statMap table. That is state: `processMod` may set `hasGlobalEffect` on the effect (8 skillStatMap mods carry `GlobalEffect`), so
//!   it is a `Cell`, and the copy is cached per statMap table ([`Skills::stat_map`]).
//! * Shared tables (Data/Skills/act_dex.lua:7324-7327): `ExplosiveTrapAltX` uses `ExplosiveTrap`'s `parts`/`statMap`/`baseMods` tables.
//!   `statMap._grantedEffect` and the `source` of the shared mods belong to whichever of the two `pairs(data.skills)` (Data.lua:1049) visits
//!   last -- per process in LuaJIT (MINEFIELD-RUST R13: 15/20 AltX). Rust takes the data order (the converter's: `ExplosiveTrapAltX` last),
//!   and models the sharing: one statMap table, its lazy copies made for `_grantedEffect` = `ExplosiveTrapAltX`.
//! * `#levels` / `next(levels)` (validateGemLevel, `naturalMaxLevel`, ProcessGemLevel): the Lua table shape is not in the data (levels are
//!   canonicalised to a sorted list) -- `data-patches/skill-levels-shape.json` (taken from LuaJIT, [`crate::patches::skill_levels_shape`]).
//! * `unsupported`: no granted effect in the data has it (`deny_unknown_fields` would reject it), so `grantedEffect.unsupported` is nil.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_mods::{Str, Table, Tag, Value};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::skill_stat_map::{ModElem, SkillMod, StatMapEntry, json_to_elems, json_to_entry, skill_stat_map};
use crate::wire::OrderedMap;

/// One `levels[level]` entry: the stat values `[1..n]` and the named fields.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillLevel {
    pub level: f64,
    pub values: Vec<f64>,
    #[serde(rename = "levelRequirement")]
    pub level_requirement: Option<f64>,
    #[serde(rename = "statInterpolation")]
    pub stat_interpolation: Option<Vec<f64>>,
    pub cost: Option<OrderedMap<f64>>,
    #[serde(rename = "critChance")]
    pub crit_chance: Option<f64>,
    #[serde(rename = "damageEffectiveness")]
    pub damage_effectiveness: Option<f64>,
    #[serde(rename = "manaMultiplier")]
    pub mana_multiplier: Option<f64>,
    #[serde(rename = "baseMultiplier")]
    pub base_multiplier: Option<f64>,
    pub cooldown: Option<f64>,
    #[serde(rename = "storedUses")]
    pub stored_uses: Option<f64>,
    #[serde(rename = "attackSpeedMultiplier")]
    pub attack_speed_multiplier: Option<f64>,
    #[serde(rename = "PvPDamageMultiplier")]
    pub pvp_damage_multiplier: Option<f64>,
    #[serde(rename = "soulPreventionDuration")]
    pub soul_prevention_duration: Option<f64>,
    #[serde(rename = "vaalStoredUses")]
    pub vaal_stored_uses: Option<f64>,
    #[serde(rename = "manaReservationPercent")]
    pub mana_reservation_percent: Option<f64>,
    #[serde(rename = "attackTime")]
    pub attack_time: Option<f64>,
    #[serde(rename = "manaReservationFlat")]
    pub mana_reservation_flat: Option<f64>,
    #[serde(rename = "lifeReservationPercent")]
    pub life_reservation_percent: Option<f64>,
    pub duration: Option<f64>,
}

impl SkillLevel {
    /// `level[index]` (1-based stat value).
    pub fn value(&self, index: usize) -> Option<f64> {
        index.checked_sub(1).and_then(|i| self.values.get(i)).copied()
    }

    /// The named numeric fields as `(name, value)` (everything but `cost`/`statInterpolation`), for dumps.
    pub fn named_numbers(&self) -> Vec<(&'static str, f64)> {
        let f = [
            ("PvPDamageMultiplier", self.pvp_damage_multiplier),
            ("attackSpeedMultiplier", self.attack_speed_multiplier),
            ("attackTime", self.attack_time),
            ("baseMultiplier", self.base_multiplier),
            ("cooldown", self.cooldown),
            ("critChance", self.crit_chance),
            ("damageEffectiveness", self.damage_effectiveness),
            ("duration", self.duration),
            ("levelRequirement", self.level_requirement),
            ("lifeReservationPercent", self.life_reservation_percent),
            ("manaMultiplier", self.mana_multiplier),
            ("manaReservationFlat", self.mana_reservation_flat),
            ("manaReservationPercent", self.mana_reservation_percent),
            ("soulPreventionDuration", self.soul_prevention_duration),
            ("storedUses", self.stored_uses),
            ("vaalStoredUses", self.vaal_stored_uses),
        ];
        f.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect()
    }
}

/// One `parts[i]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillPart {
    pub name: String,
    pub area: Option<bool>,
    pub projectile: Option<bool>,
    pub melee: Option<bool>,
    pub stages: Option<bool>,
    pub cast: Option<bool>,
    pub spell: Option<bool>,
    pub chaining: Option<bool>,
    #[serde(rename = "channelRelease")]
    pub channel_release: Option<bool>,
    pub attack: Option<bool>,
    pub hit: Option<bool>,
}

/// The raw `skills.json` record.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEffect {
    id: String,
    name: String,
    mod_source: String,
    levels: Vec<SkillLevel>,
    stats: Vec<String>,
    stat_map: serde_json::Map<String, Json>,
    hooks: Vec<String>,
    color: Option<f64>,
    stat_description_scope: Option<String>,
    description: Option<String>,
    constant_stats: Option<Vec<(String, f64)>>,
    base_flags: Option<Vec<String>>,
    skill_types: Option<Vec<u32>>,
    cast_time: Option<f64>,
    quality_stats: Option<Vec<(String, f64)>>,
    base_effectiveness: Option<f64>,
    hidden: Option<bool>,
    not_minion_stat: Option<Vec<String>>,
    base_type_name: Option<String>,
    incremental_effectiveness: Option<f64>,
    base_mods: Option<Json>,
    weapon_types: Option<OrderedMap<bool>>,
    add_skill_types: Option<Vec<f64>>,
    exclude_skill_types: Option<Vec<f64>>,
    require_skill_types: Option<Vec<f64>>,
    support: Option<bool>,
    has_global_effect: Option<bool>,
    from_item: Option<bool>,
    parts: Option<Vec<SkillPart>>,
    minion_list: Option<Vec<String>>,
    cannot_be_supported: Option<bool>,
    ignore_minion_types: Option<bool>,
    minion_skill_types: Option<OrderedMap<bool>>,
    flavour_text: Option<Vec<String>>,
    plus_version_of: Option<String>,
    is_trigger: Option<bool>,
    legacy: Option<bool>,
    from_tree: Option<bool>,
    skill_totem_id: Option<f64>,
    minion_uses: Option<OrderedMap<bool>>,
    add_flags: Option<OrderedMap<bool>>,
    support_gems_only: Option<bool>,
    minion_has_item_set: Option<bool>,
    level_mods: Option<Json>,
    hide_from_gem_list: Option<bool>,
    add_minion_list: Option<Vec<String>>,
}

/// `skillTypes[SkillType.X] = true` -- a set of SkillType numbers.
pub type SkillTypeSet = BTreeSet<u32>;

/// A granted effect (`data.skills[id]`). Fields not listed in the R6 calc stay in the raw data (`description`, `flavourText`, ...).
#[derive(Debug)]
pub struct GrantedEffect {
    /// Index in [`Skills::effects`] (the identity Lua compares tables by).
    pub index: usize,
    pub id: String,
    pub name: String,
    pub mod_source: String,
    /// Sorted by `level` (the converter's canonical form); [`GrantedEffect::level`] is `levels[k]`.
    pub levels: Vec<SkillLevel>,
    /// `#grantedEffect.levels` in LuaJIT (border of the table, `data-patches/skill-levels-shape.json`).
    pub levels_len: usize,
    /// `next(grantedEffect.levels)` -- the first key LuaJIT's `next` returns.
    pub levels_next: f64,
    pub stats: Vec<String>,
    pub constant_stats: Option<Vec<(String, f64)>>,
    pub quality_stats: Option<Vec<(String, f64)>>,
    pub base_mods: Option<Vec<ModElem>>,
    pub level_mods: Option<Vec<ModElem>>,
    pub parts: Option<Vec<SkillPart>>,
    pub hooks: Vec<String>,
    pub color: Option<f64>,
    pub support: bool,
    pub skill_types: Option<SkillTypeSet>,
    pub minion_skill_types: Option<SkillTypeSet>,
    pub add_skill_types: Option<Vec<f64>>,
    pub exclude_skill_types: Option<Vec<f64>>,
    pub require_skill_types: Option<Vec<f64>>,
    pub weapon_types: Option<OrderedMap<bool>>,
    pub base_flags: Option<Vec<String>>,
    pub add_flags: Option<OrderedMap<bool>>,
    pub minion_uses: Option<OrderedMap<bool>>,
    pub minion_list: Option<Vec<String>>,
    pub add_minion_list: Option<Vec<String>>,
    pub not_minion_stat: Option<Vec<String>>,
    pub plus_version_of: Option<String>,
    pub base_type_name: Option<String>,
    pub stat_description_scope: Option<String>,
    pub cast_time: Option<f64>,
    pub base_effectiveness: Option<f64>,
    pub incremental_effectiveness: Option<f64>,
    pub skill_totem_id: Option<f64>,
    pub hidden: bool,
    pub from_item: bool,
    pub from_tree: bool,
    pub cannot_be_supported: bool,
    pub ignore_minion_types: bool,
    pub is_trigger: bool,
    pub legacy: bool,
    pub support_gems_only: bool,
    pub minion_has_item_set: bool,
    pub hide_from_gem_list: bool,
    /// `grantedEffect.hasGlobalEffect` -- set by `processMod`, also during a lazy statMap copy.
    pub has_global_effect: Cell<bool>,
    /// Index of this effect's statMap TABLE in [`Skills::stat_maps`] (shared tables share it).
    pub stat_map_table: usize,
}

impl GrantedEffect {
    /// `grantedEffect.levels[k]`.
    pub fn level(&self, k: f64) -> Option<&SkillLevel> {
        self.levels.iter().find(|l| l.level == k)
    }

    /// `grantedEffect.skillTypes[t]` (nil skillTypes -> the Lua error of indexing nil is the caller's).
    pub fn has_skill_type(&self, t: u32) -> bool {
        self.skill_types.as_ref().is_some_and(|s| s.contains(&t))
    }
}

/// One statMap TABLE: its entries (own + lazily copied) and `_grantedEffect`.
#[derive(Debug)]
pub struct StatMapTable {
    /// `statMap._grantedEffect` (an effect index).
    pub granted_effect: usize,
    /// Own entries (already `processMod`ed by Data.lua) and lazy copies, by stat. `None` = looked up, absent in both maps.
    entries: RefCell<HashMap<String, Option<Rc<StatMapEntry>>>>,
    own: IndexMap<String, Rc<StatMapEntry>>,
}

/// `data.skills`.
#[derive(Debug)]
pub struct Skills {
    pub effects: Vec<GrantedEffect>,
    pub index: HashMap<String, usize>,
    pub stat_maps: Vec<StatMapTable>,
}

/// Data/Skills/act_dex.lua:7324-7327: `ExplosiveTrapAltX` takes `parts`/`statMap`/`baseMods` of `ExplosiveTrap` -- one Lua table.
pub const SHARED_TABLES: [(&str, &str); 1] = [("ExplosiveTrapAltX", "ExplosiveTrap")];

/// `SkillType.Buff` (Data/Global.lua; processMod's `skillTypes[SkillType.Buff]`).
fn skill_type(name: &str) -> u32 {
    let v = crate::globals::globals()
        .skill_type
        .get(name)
        .copied()
        .unwrap_or_else(|| panic!("pob-data: SkillType.{name} missing"));
    v as u32
}

impl Skills {
    pub fn get(&self, id: &str) -> Option<&GrantedEffect> {
        self.index.get(id).map(|&i| &self.effects[i])
    }

    /// `grantedEffect.statMap[stat]` with the lazy `skillStatMapMeta.__index` (Data.lua:1033-1045): an own entry, else a `copyTable` of
    /// `data.skillStatMap[stat]` with `processMod(t._grantedEffect, mod, stat)` on every mod, stored in the table; else nil.
    /// `Err` is the Lua error `processMod` raises on a malformed mod (the access itself fails in LuaJIT).
    pub fn stat_map(&self, effect: &GrantedEffect, stat: &str) -> Result<Option<Rc<StatMapEntry>>, String> {
        let table = &self.stat_maps[effect.stat_map_table];
        if let Some(e) = table.entries.borrow().get(stat) {
            return Ok(e.clone());
        }
        if let Some(e) = table.own.get(stat) {
            let e = Some(e.clone());
            table.entries.borrow_mut().insert(stat.to_owned(), e.clone());
            return Ok(e);
        }
        let ssm = skill_stat_map();
        let Some(map) = ssm.entries.get(stat) else {
            // `__index` returns nil without storing anything
            return Ok(None);
        };
        let mut copy = map.deep_copy();
        let owner = &self.effects[table.granted_effect];
        for elem in &mut copy.mods {
            for m in elem.mods_mut() {
                process_mod(owner, m, Some(stat))?;
            }
        }
        let e = Some(Rc::new(copy));
        table.entries.borrow_mut().insert(stat.to_owned(), e.clone());
        Ok(e)
    }
}

/// Modules/Data.lua:68-91 `processMod(grantedEffect, mod, statName)`.
pub fn process_mod(ge: &GrantedEffect, m: &mut SkillMod, stat_name: Option<&str>) -> Result<(), String> {
    let (source, value, tags_global) = match m {
        SkillMod::Mod(md) => (&mut md.source, &mut md.value, md.tags.iter().any(|t| t.ty().is_str("GlobalEffect"))),
        SkillMod::Malformed(mm) => {
            if let Some(e) = mm.process_mod_error() {
                mm.source = Some(Str::from(ge.mod_source.as_str()));
                return Err(e);
            }
            let g = mm
                .array
                .iter()
                .any(|v| matches!(v, Value::Table(t) if t.get("type").is_str("GlobalEffect")));
            (&mut mm.source, &mut mm.value, g)
        }
    };
    *source = Some(Str::from(ge.mod_source.as_str()));
    // `if type(mod.value) == "table" and mod.value.mod then mod.value.mod.source = "Skill:"..grantedEffect.id end`
    if let Value::Table(t) = value
        && t.get("mod").truthy()
    {
        let t = Rc::make_mut(t);
        let src = Str::from(format!("Skill:{}", ge.id).as_str());
        match t.hash.get_mut("mod") {
            Some(Value::Mod(inner)) => Rc::make_mut(inner).source = Some(src),
            Some(v @ Value::Table(_)) => v.set("source", Value::Str(src)),
            _ => unimplemented!("Modules/Data.lua:71 `mod.value.mod.source = ...` on a non-table `value.mod`"),
        }
    }
    if tags_global {
        ge.has_global_effect.set(true);
    }
    let not_minion = match (&ge.not_minion_stat, stat_name) {
        (Some(list), Some(stat)) if ge.support || ge.has_skill_type(skill_type("Buff")) => list.iter().any(|s| s == stat),
        _ => false,
    };
    if not_minion {
        let mut t = Table::default();
        t.hash.insert(Str::from("type"), Value::str("ActorCondition"));
        t.hash.insert(Str::from("actor"), Value::str("parent"));
        t.hash.insert(Str::from("neg"), Value::Bool(true));
        match m {
            SkillMod::Mod(md) => md.tags.push(Tag::new(t)),
            SkillMod::Malformed(mm) => mm.array.push(Value::table(t)),
        }
    }
    Ok(())
}

fn type_set(v: Option<Vec<u32>>) -> Option<SkillTypeSet> {
    v.map(|v| v.into_iter().collect())
}

fn key_set(v: Option<OrderedMap<bool>>, what: &str) -> Result<Option<SkillTypeSet>, String> {
    v.map(|m| {
        m.iter()
            .map(|(k, b)| match (k.parse::<u32>(), b) {
                (Ok(n), true) => Ok(n),
                _ => Err(format!("{what}: key '{k}' = {b}")),
            })
            .collect()
    })
    .transpose()
}

fn build(raw: OrderedMap<RawEffect>, shapes: &HashMap<String, (usize, f64)>) -> Result<Skills, String> {
    let mut effects = Vec::with_capacity(raw.len());
    let mut index = HashMap::with_capacity(raw.len());
    let mut stat_maps = Vec::with_capacity(raw.len());
    for (key, r) in raw.0 {
        if r.id != key {
            return Err(format!("skills: '{key}' has id '{}'", r.id));
        }
        let i = effects.len();
        let ctx = |e: String| format!("skills '{key}': {e}");
        let mut own = IndexMap::with_capacity(r.stat_map.len());
        for (stat, v) in &r.stat_map {
            own.insert(
                stat.clone(),
                Rc::new(json_to_entry(v).map_err(|e| ctx(format!("statMap.{stat}: {e}")))?),
            );
        }
        stat_maps.push(StatMapTable {
            granted_effect: i,
            entries: RefCell::new(HashMap::new()),
            own,
        });
        let &(levels_len, levels_next) = shapes
            .get(&key)
            .ok_or_else(|| ctx("no levels shape in data-patches/skill-levels-shape.json".into()))?;
        effects.push(GrantedEffect {
            index: i,
            name: r.name,
            mod_source: r.mod_source,
            levels: r.levels,
            levels_len,
            levels_next,
            stats: r.stats,
            constant_stats: r.constant_stats,
            quality_stats: r.quality_stats,
            base_mods: r
                .base_mods
                .as_ref()
                .map(json_to_elems)
                .transpose()
                .map_err(|e| ctx(format!("baseMods: {e}")))?,
            level_mods: r
                .level_mods
                .as_ref()
                .map(json_to_elems)
                .transpose()
                .map_err(|e| ctx(format!("levelMods: {e}")))?,
            parts: r.parts,
            hooks: r.hooks,
            color: r.color,
            support: r.support.unwrap_or(false),
            skill_types: type_set(r.skill_types),
            minion_skill_types: key_set(r.minion_skill_types, "minionSkillTypes").map_err(ctx)?,
            add_skill_types: r.add_skill_types,
            exclude_skill_types: r.exclude_skill_types,
            require_skill_types: r.require_skill_types,
            weapon_types: r.weapon_types,
            base_flags: r.base_flags,
            add_flags: r.add_flags,
            minion_uses: r.minion_uses,
            minion_list: r.minion_list,
            add_minion_list: r.add_minion_list,
            not_minion_stat: r.not_minion_stat,
            plus_version_of: r.plus_version_of,
            base_type_name: r.base_type_name,
            stat_description_scope: r.stat_description_scope,
            cast_time: r.cast_time,
            base_effectiveness: r.base_effectiveness,
            incremental_effectiveness: r.incremental_effectiveness,
            skill_totem_id: r.skill_totem_id,
            hidden: r.hidden.unwrap_or(false),
            from_item: r.from_item.unwrap_or(false),
            from_tree: r.from_tree.unwrap_or(false),
            cannot_be_supported: r.cannot_be_supported.unwrap_or(false),
            ignore_minion_types: r.ignore_minion_types.unwrap_or(false),
            is_trigger: r.is_trigger.unwrap_or(false),
            legacy: r.legacy.unwrap_or(false),
            support_gems_only: r.support_gems_only.unwrap_or(false),
            minion_has_item_set: r.minion_has_item_set.unwrap_or(false),
            hide_from_gem_list: r.hide_from_gem_list.unwrap_or(false),
            has_global_effect: Cell::new(r.has_global_effect.unwrap_or(false)),
            stat_map_table: i,
            id: key.clone(),
        });
        let _ = (r.description, r.flavour_text);
        index.insert(key, i);
    }
    if shapes.len() != effects.len() {
        return Err(format!(
            "skill-levels-shape.json has {} shapes for {} effects",
            shapes.len(),
            effects.len()
        ));
    }
    // Shared tables: the user takes the owner's statMap TABLE; `_grantedEffect` = the one `pairs` visits last in data order.
    for (user, owner) in SHARED_TABLES {
        let (u, o) = (index[user], index[owner]);
        let same = |a: &Option<Vec<ModElem>>, b: &Option<Vec<ModElem>>| format!("{a:?}") == format!("{b:?}");
        if !same(&effects[u].base_mods, &effects[o].base_mods) || format!("{:?}", stat_maps[u].own) != format!("{:?}", stat_maps[o].own) {
            return Err(format!(
                "skills: '{user}' no longer shares '{owner}''s tables (act_dex.lua:7324-7327)"
            ));
        }
        effects[u].stat_map_table = o;
        stat_maps[o].granted_effect = u.max(o);
    }
    Ok(Skills { effects, index, stat_maps })
}

thread_local! {
    static SKILLS: OnceCell<Rc<Skills>> = const { OnceCell::new() };
}

/// `data.skills` of this thread (mods hold `Rc`s; the lazy statMap is per-thread state, like LuaJIT's per-process `data`).
pub fn skills() -> Rc<Skills> {
    SKILLS.with(|c| {
        c.get_or_init(|| {
            let raw: OrderedMap<RawEffect> = crate::load("skills.json");
            let shapes = crate::patches::skill_levels_shape().unwrap_or_else(|e| panic!("{e}"));
            Rc::new(build(raw, shapes).unwrap_or_else(|e| panic!("pob-data: {e}")))
        })
        .clone()
    })
}

/// `grantedEffect.description` of the effects that have one (the model above keeps only what the calculation reads). Thread-free of [`skills`]: a reader on its own thread can call it.
pub fn skill_descriptions() -> Result<HashMap<String, String>, crate::DataError> {
    #[derive(Deserialize)]
    struct Row {
        description: Option<String>,
    }
    let rows: HashMap<String, Row> = crate::read_json("skills.json")?;
    Ok(rows.into_iter().filter_map(|(k, r)| Some((k, r.description?))).collect())
}

/// A fresh `data.skills` (no lazy copies made yet) -- for tests that need the state of a new process.
pub fn load_skills_fresh() -> Result<Skills, crate::DataError> {
    let raw: OrderedMap<RawEffect> = crate::read_json("skills.json")?;
    let shapes = crate::patches::skill_levels_shape()?;
    build(raw, shapes).map_err(|e| crate::DataError(format!("pob-data: {e}")))
}
