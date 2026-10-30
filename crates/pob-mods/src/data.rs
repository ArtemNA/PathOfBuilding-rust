//! The few `data.*` tables the mod core reads (orchestrator decision 6), loaded lazily from this repo's `data/` (synced from
//! pob-ts by `tools/sync-data.ps1`): `highPrecisionMods`/`defaultHighPrecision` (rules.json, Data.lua:434-470), the gem
//! lookups of CalcTools.lua:164-176, 279-292 (gems.json), `KeywordFlag.MatchAll` and `SkillType.HasReservation`
//! (globals.json). Only lookups -- nothing here is iterated, so std `HashMap` is fine (RUST-ARCH §4).
//! Full loaders are R3 (`pob-data`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::Value as Json;

/// `POB_DATA_DIR`, or `<workspace root>/data`.
pub fn data_dir() -> PathBuf {
    match std::env::var_os("POB_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data")),
    }
}

fn load(file: &str) -> Json {
    let path = data_dir().join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("pob-mods data: {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("pob-mods data: {}: {e}", path.display()))
}

fn num(v: &Json, what: &str) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("pob-mods data: {what} is not a number"))
}

/// The mod-core rules: `highPrecisionMods[name][type]`, `defaultHighPrecision`, and two flag constants.
pub struct ModRules {
    pub high_precision_mods: HashMap<String, HashMap<String, f64>>,
    pub default_high_precision: f64,
    /// `KeywordFlag.MatchAll` (Data/Global.lua).
    pub match_all: u32,
    /// `SkillType.HasReservation` (Data/Global.lua).
    pub has_reservation: f64,
}

pub fn rules() -> &'static ModRules {
    static R: OnceLock<ModRules> = OnceLock::new();
    R.get_or_init(|| {
        let rules = load("rules.json");
        let globals = load("globals.json");
        let mut hp = HashMap::new();
        for (name, types) in rules["highPrecisionMods"].as_object().expect("rules.highPrecisionMods") {
            let per: HashMap<String, f64> = types
                .as_object()
                .expect("highPrecisionMods entry")
                .iter()
                .map(|(t, p)| (t.clone(), num(p, "precision")))
                .collect();
            hp.insert(name.clone(), per);
        }
        ModRules {
            high_precision_mods: hp,
            default_high_precision: num(&rules["defaultHighPrecision"], "defaultHighPrecision"),
            match_all: num(&globals["KeywordFlag"]["MatchAll"], "KeywordFlag.MatchAll") as u32,
            has_reservation: num(&globals["SkillType"]["HasReservation"], "SkillType.HasReservation"),
        }
    })
}

/// `data.highPrecisionMods[name] and data.highPrecisionMods[name][type]`.
pub fn high_precision(name: &str, ty: &str) -> Option<f64> {
    rules().high_precision_mods.get(name).and_then(|m| m.get(ty)).copied()
}

/// One `data.gems[id]`: only what `getGameIdFromGemName` reads.
pub struct GemEntry {
    pub game_id: String,
    pub vaal_gem: bool,
}

pub struct GemData {
    /// `data.gemForBaseName` -- lowercase gem name -> gem id.
    pub gem_for_base_name: HashMap<String, String>,
    pub gems: HashMap<String, GemEntry>,
    /// `data.gemVaalGemIdForBaseGemId` -- despite the name, VAAL gem id -> base gem id (MOD-CORE §7.13).
    pub gem_vaal_gem_id_for_base_gem_id: HashMap<String, String>,
}

fn str_map(v: &Json, what: &str) -> HashMap<String, String> {
    v.as_object()
        .unwrap_or_else(|| panic!("pob-mods data: {what}"))
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_str().unwrap_or_else(|| panic!("pob-mods data: {what}.{k}")).to_owned(),
            )
        })
        .collect()
}

pub fn gems() -> &'static GemData {
    static G: OnceLock<GemData> = OnceLock::new();
    G.get_or_init(|| {
        let j = load("gems.json");
        let gems = j["gems"]
            .as_object()
            .expect("gems.gems")
            .iter()
            .map(|(id, g)| {
                let game_id = g["gameId"].as_str().unwrap_or_else(|| panic!("gems[{id}].gameId")).to_owned();
                (
                    id.clone(),
                    GemEntry {
                        game_id,
                        vaal_gem: g["vaalGem"].as_bool() == Some(true),
                    },
                )
            })
            .collect();
        GemData {
            gem_for_base_name: str_map(&j["gemForBaseName"], "gemForBaseName"),
            gems,
            gem_vaal_gem_id_for_base_gem_id: str_map(&j["gemVaalGemIdForBaseGemId"], "gemVaalGemIdForBaseGemId"),
        }
    })
}
