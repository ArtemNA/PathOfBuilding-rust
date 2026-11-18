//! data/manifest.json: the identity of a data set -- upstream sha, converter version, tree versions and every data file
//! with its byte size, sha256 and the `data.*` sources it was built from. No timestamps (determinism). Written last, from
//! the files on disk, so it hashes exactly the bytes that were written.
//!
//! The file list and order are the old pipeline's extractor registry (extract/index.ts EXTRACTORS, then one entry per
//! tree version and legion). `sources` is documentation carried over verbatim from each extractor.

use std::path::Path;

use crate::ctx::{Ctx, Obj, R};
use crate::js::J;
use crate::trees::{Versions, out_file};

/// Bump on any change that alters the produced JSON (the old CONVERTER_VERSION, kept so manifests stay comparable).
pub const CONVERTER_VERSION: &str = "0.1.0";

const ITEM_MOD_CATEGORIES: &[&str] = &[
    "Explicit", "ItemExclusive", "Corrupted", "Delve", "Synthesis", "Scourge", "Eldritch", "Mercenary", "Flask", "Tincture", "Graft",
    "Jewel", "JewelAbyss", "JewelCluster", "JewelCharm", "Foulborn", "WatchersEye",
];

const CONSTANTS_SOURCES: &[&str] = &[
    "misc", "characterConstants", "monsterConstants", "gameConstants", "monsterAccuracyTable", "monsterAilmentThresholdTable",
    "monsterAllyDamageTable", "monsterAllyLifeTable", "monsterArmourTable", "monsterDamageTable", "monsterEvasionTable",
    "monsterLifeTable", "monsterLifeTable2", "monsterLifeTable3", "monsterPhysConversionMultiTable", "monsterExperienceLevelMap",
    "mapLevelBossAilmentMult", "mapLevelBossLifeMult", "mapLevelLifeMult", "monsterVarietyLifeMult", "totemLifeMult",
    "goldRespecPrices",
];

const RULES_SOURCES: &[&str] = &[
    "weaponTypeInfo", "unarmedWeaponData", "nonDamagingAilment", "ailmentTypeList", "elementalAilmentTypeList",
    "nonDamagingAilmentTypeList", "nonElementalAilmentTypeList", "highPrecisionMods", "defaultHighPrecision", "cursePriority",
    "keystones", "jewelRadii", "skillColorMap", "enchantmentSource", "itemTagSpecial", "itemTagSpecialExclusionPattern",
    "timelessJewelTypes", "timelessJewelSeedMin", "timelessJewelSeedMax", "timelessJewelTradeIDs", "timelessJewelAdditions",
    "casterTagCrucibleUniques", "minionTagCrucibleUniques", "vestigialUniqueBaseTypes", "vestigialModMappings", "bossStats",
    "enemyIsBossTooltip", "powerStatList",
];

/// (file relative to data/, sources) in extractor registry order.
fn static_files(has_viscera: bool) -> Vec<(String, Vec<&'static str>)> {
    let f = |file: &str, src: &[&'static str]| (file.to_owned(), src.to_vec());
    let mut v = vec![
        f("globals.json", &["_globals"]),
        f("constants.json", CONSTANTS_SOURCES),
        f("rules.json", RULES_SOURCES),
        f("costs.json", &["costs"]),
        f("item-bases.json", &["itemBases", "itemBaseLists", "itemBaseTypeList"]),
    ];
    for cat in ITEM_MOD_CATEGORIES {
        v.push(f(&format!("item-mods/{cat}.json"), &["itemMods"]));
    }
    v.extend([
        f("item-mods/_index.json", &["itemMods"]),
        f("master-mods.json", &["masterMods"]),
        f("veiled-mods.json", &["veiledMods"]),
        f("essences.json", &["essences"]),
        f("necropolis-mods.json", &["necropolisMods"]),
        f("enchantments.json", &["enchantments"]),
        f("mod-scalability.json", &["modScalability"]),
        f("skills.json", &["skills"]),
        f("skill-stat-map.json", &["skillStatMap"]),
        f(
            "gems.json",
            &["gems", "gemForSkill", "gemForBaseName", "gemsByGameId", "gemGrantedEffectIdForVaalGemId", "gemVaalGemIdForBaseGemId", "skills"],
        ),
        f("uniques.json", &["uniques", "uniqueMods"]),
        f("rares.json", &["rares"]),
    ]);
    // uniques.ts `when`: only for an upstream that has data.viscera.
    if has_viscera {
        v.push(f("viscera.json", &["viscera"]));
    }
    v.extend([
        f("rare-like-uniques.json", &["rareLikeUniques", "itemMods", "veiledMods", "itemBaseLists"]),
        f("minions.json", &["minions", "spectres"]),
        f("bosses.json", &["bosses"]),
        f("boss-skills.json", &["bossSkills", "bossSkillsList"]),
        f("pantheons.json", &["pantheons"]),
        f("map-mods.json", &["mapMods"]),
        f("cluster-jewels.json", &["clusterJewels", "clusterJewelInfoForNotable"]),
        f("foulborn-map.json", &["foulbornMap"]),
        f("abyss-notable-names.json", &["abyssNotableNames"]),
        f("node-id-list.json", &["nodeIDList"]),
        f("mod-cache.json", &["_modCache"]),
        f("mod-parser.json", &["_modParser"]),
        f("config-options.json", &["_configOptions"]),
        f("closures.json", &["*"]),
    ]);
    v
}

/// `git rev-parse HEAD` of the checkout (read-only); "unknown" without git metadata, like the old pipeline.
fn upstream_sha(pob: &Path) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(pob)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}

/// `c` is the main dump's context (for `data.viscera`); `out` holds every data file already written.
pub fn convert(c: &mut Ctx, vers: &Versions, out: &Path) -> R<J> {
    let data = c.d.root("data");
    let has_viscera = c.d.t(c.table(data, "data")?).get("viscera").is_some();
    let mut files = static_files(has_viscera);
    for ver in &vers.list {
        files.push((out_file(ver, &vers.latest), vec![]));
    }
    files.push(("tree/legion.json".into(), vec![]));

    let mut list = Vec::with_capacity(files.len());
    let mut missing = Vec::new();
    for (file, sources) in &files {
        let sources: Vec<J> = if file.starts_with("tree/") {
            let id = file.trim_start_matches("tree/").trim_start_matches("_ondemand/").trim_end_matches(".json");
            vec![J::Str(format!("tree/{id}"))]
        } else {
            sources.iter().map(|s| J::str(s)).collect()
        };
        let bytes = match std::fs::read(out.join(file)) {
            Ok(b) => b,
            Err(_) => {
                missing.push(file.clone());
                continue;
            }
        };
        let mut o = Obj::default();
        o.put("file", J::Str(file.clone()));
        o.put("bytes", J::Num(bytes.len() as f64));
        o.put("sha256", J::Str(pob_data::sha256::sha256_hex(&bytes)));
        o.put("sources", J::Arr(sources));
        list.push(o.done());
    }
    if !missing.is_empty() {
        return Err(format!("manifest.json: {} data file(s) not in the output dir: {}", missing.len(), missing.join(", ")));
    }
    c.count("manifest.json", "files", list.len());
    let mut m = Obj::default();
    m.put("pobSha", J::Str(upstream_sha(&c.pob_root)));
    m.put("converterVersion", J::str(CONVERTER_VERSION));
    m.put("latestTreeVersion", J::str(&vers.latest));
    m.put("treeVersionList", J::Arr(vers.list.iter().map(|s| J::str(s)).collect()));
    m.put("files", J::Arr(list));
    Ok(m.done())
}
