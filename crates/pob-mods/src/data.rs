//! The few `data.*` tables the mod core reads (orchestrator decision 6), loaded lazily from this repo's `data/` (synced from
//! pob-ts by `tools/sync-data.ps1`): `highPrecisionMods`/`defaultHighPrecision` (rules.json, Data.lua:434-470), the gem
//! lookups of CalcTools.lua:164-176, 279-292 (gems.json), `KeywordFlag.MatchAll` and `SkillType.HasReservation`
//! (globals.json). Only lookups -- nothing here is iterated, so std `HashMap` is fine (RUST-ARCH §4).
//! Full loaders are R3 (`pob-data`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value as Json;

/// `<dir>/data` when it holds the converted data.
fn data_under(dir: &Path) -> Option<PathBuf> {
    let d = dir.join("data");
    d.join("manifest.json").is_file().then_some(d)
}

/// `<folder of the exe>/data` (the package), then `POB_DATA_DIR`, then in a development tree the `data/` nearest above the exe,
/// or above `CARGO_MANIFEST_DIR` of a cargo run with its target folder elsewhere. No path of the build machine goes into the binary.
pub fn data_dir() -> PathBuf {
    static EXE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    let exe_dir = EXE_DIR.get_or_init(|| std::env::current_exe().ok()?.parent().map(Path::to_path_buf));
    if let Some(d) = exe_dir.as_deref().and_then(data_under) {
        return d;
    }
    if let Some(d) = std::env::var_os("POB_DATA_DIR") {
        return PathBuf::from(d);
    }
    let above = |dir: &Path| dir.ancestors().find_map(data_under);
    exe_dir
        .as_deref()
        .and_then(above)
        .or_else(|| std::env::var_os("CARGO_MANIFEST_DIR").and_then(|d| above(Path::new(&d))))
        .unwrap_or_else(|| PathBuf::from("data"))
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
