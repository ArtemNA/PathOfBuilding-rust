//! R6.2 gate against the LuaJIT probe `tools/lua-corpus/export-gemdata.ts` (fixtures `crates/pob-data/tests/fixtures/gemdata-*.json`, 20
//! LuaJIT processes per part). Where LuaJIT itself differs between processes (MINEFIELD-RUST R13, class `lj-strhash`) the fixture holds
//! `{"$oneOf": [...]}` and Rust (data order) must be one of them; the FindSkillGem "Ambiguous" text is checked against the order-free set
//! of matches.
//!   (1) data: `data.gems` and its lookups, every granted effect (levels, stats, mods, the RESOLVED statMap incl. the lazy copies);
//!   (2) CalcTools helpers + ProcessGemLevel + FindSkillGem; L24 malformed mods;
//!   (3) `build.skillsTab` of the 6 goldens right after SkillsTab:Load.

use std::collections::BTreeMap;

use pob_calc::calc_tools::{
    ActiveSkillView, SkillInstance, WeaponView, build_skill_instance_stats, can_granted_effect_support_active_skill,
    does_type_expression_match, get_gem_stat_requirement,
};
use pob_calc::skills_tab::{GemInstance, SkillsTab, find_skill_gem, load_skills_tab, process_gem_level};
use pob_data::gems::gems;
use pob_data::sha256::sha256_hex;
use pob_data::skill_stat_map::{
    FORMAT_FLAGS_TABLE_ERROR, FORMAT_TAGS_NUMBER_ERROR, ModElem, PROCESS_MOD_NUMBER_TAG_ERROR, QUERY_FLAGS_TABLE_ERROR,
    QUERY_TAGS_NUMBER_ERROR, SkillMod, StatMapEntry, skill_stat_map,
};
use pob_data::skills::{GrantedEffect, SkillLevel, Skills, load_skills_fresh, skills};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::format::format_source_mod;
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pob-data/tests/fixtures")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

/// The probe's `N(x)`: `%.17g` for numbers.
fn n17(x: f64) -> String {
    format_g(x, 17)
}

/// Rust value `got` against a fixture value that may be `{"$oneOf": [...]}` (R13).
fn matches_lj(want: &Json, got: &Json) -> bool {
    match want.get("$oneOf").and_then(Json::as_array) {
        Some(alts) => alts.iter().any(|a| a == got),
        None => want == got,
    }
}

fn is_one_of(want: &Json) -> bool {
    want.get("$oneOf").is_some()
}

// --- formatting as the probe ---------------------------------------------------------------------------------------------------

fn fmt_mod(m: &SkillMod) -> String {
    match m {
        SkillMod::Mod(m) => format_source_mod(m),
        SkillMod::Malformed(mm) => format!("ERR:{}", mm.format_error()),
    }
}

fn fmt_elem(e: &ModElem) -> String {
    match e {
        ModElem::Mod(m) => fmt_mod(m),
        ModElem::Group { mods, div, mult } => {
            let mut s = format!("{{{}}}", mods.iter().map(fmt_mod).collect::<Vec<_>>().join(";"));
            if let Some(d) = div {
                s += &format!("div={}", n17(*d));
            }
            if let Some(m) = mult {
                s += &format!("mult={}", n17(*m));
            }
            s
        }
    }
}

fn fmt_list(l: &Option<Vec<ModElem>>) -> Json {
    match l {
        None => Json::Null,
        Some(l) => json!(l.iter().map(fmt_elem).collect::<Vec<_>>()),
    }
}

fn fmt_entry(e: &Option<std::rc::Rc<StatMapEntry>>) -> Json {
    let Some(e) = e else { return Json::Bool(false) };
    let mut out: Vec<String> = e.mods.iter().map(fmt_elem).collect();
    for (k, v) in [("base", e.base), ("div", e.div), ("mult", e.mult)] {
        if let Some(v) = v {
            out.push(format!("{k}={}", n17(v)));
        }
    }
    if let Some(f) = &e.skill_flag {
        out.push(format!("skillFlag={f}"));
    }
    if let Some(v) = e.value {
        out.push(format!("value={}", n17(v)));
    }
    json!(out)
}

fn level_text(l: &SkillLevel) -> String {
    let mut s = l.values.iter().map(|v| n17(*v)).collect::<Vec<_>>().join(",");
    let mut named: Vec<(String, String)> = l.named_numbers().into_iter().map(|(k, v)| (k.to_owned(), n17(v))).collect();
    if let Some(c) = &l.cost {
        let mut kv: Vec<(&String, &f64)> = c.iter().collect();
        kv.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        named.push((
            "cost".into(),
            format!(
                "{{{}}}",
                kv.iter().map(|(k, v)| format!("{k}={}", n17(**v))).collect::<Vec<_>>().join(",")
            ),
        ));
    }
    if let Some(si) = &l.stat_interpolation {
        let parts: Vec<String> = si.iter().enumerate().map(|(i, v)| format!("{}={}", i + 1, n17(*v))).collect();
        named.push(("statInterpolation".into(), format!("{{{}}}", parts.join(","))));
    }
    named.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for (k, v) in named {
        s += &format!(";{k}={v}");
    }
    s
}

fn levels_text(ge: &GrantedEffect) -> String {
    ge.levels
        .iter()
        .map(|l| format!("{}:{}", n17(l.level), level_text(l)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pair_list(l: &Option<Vec<(String, f64)>>) -> Json {
    match l {
        None => Json::Null,
        Some(l) => json!(l.iter().map(|(s, v)| format!("{s}={}", n17(*v))).collect::<Vec<_>>()),
    }
}

fn num_list(l: &Option<Vec<f64>>) -> Json {
    match l {
        None => Json::Null,
        Some(l) => json!(l.iter().map(|v| n17(*v)).collect::<Vec<_>>()),
    }
}

fn set_list(s: Option<Vec<String>>) -> Json {
    match s {
        None => Json::Null,
        Some(v) => json!(v.into_iter().map(|k| format!("{k}=true")).collect::<Vec<_>>()),
    }
}

/// The probe's per-effect record (without `levels`/`statMap`, compared apart).
fn effect_record(ge: &GrantedEffect) -> Map<String, Json> {
    let mut m = Map::new();
    let mut sc: Vec<String> = Vec::new();
    let b = |k: &str, v: bool, sc: &mut Vec<String>| {
        if v {
            sc.push(format!("{k}=true"));
        }
    };
    sc.push(format!("name={}", ge.name));
    if let Some(c) = ge.color {
        sc.push(format!("color={}", n17(c)));
    }
    b("support", ge.support, &mut sc);
    if let Some(p) = &ge.plus_version_of {
        sc.push(format!("plusVersionOf={p}"));
    }
    b("cannotBeSupported", ge.cannot_be_supported, &mut sc);
    b("supportGemsOnly", ge.support_gems_only, &mut sc);
    b("fromItem", ge.from_item, &mut sc);
    b("fromTree", ge.from_tree, &mut sc);
    b("ignoreMinionTypes", ge.ignore_minion_types, &mut sc);
    b("isTrigger", ge.is_trigger, &mut sc);
    if let Some(v) = ge.base_effectiveness {
        sc.push(format!("baseEffectiveness={}", n17(v)));
    }
    if let Some(v) = ge.incremental_effectiveness {
        sc.push(format!("incrementalEffectiveness={}", n17(v)));
    }
    sc.push(format!("modSource={}", ge.mod_source));
    sc.push(format!("id={}", ge.id));
    b("hidden", ge.hidden, &mut sc);
    if let Some(v) = ge.cast_time {
        sc.push(format!("castTime={}", n17(v)));
    }
    b("legacy", ge.legacy, &mut sc);
    if let Some(v) = ge.skill_totem_id {
        sc.push(format!("skillTotemId={}", n17(v)));
    }
    m.insert("scalars".into(), json!(sc));
    m.insert("stats".into(), json!(ge.stats));
    m.insert("constantStats".into(), pair_list(&ge.constant_stats));
    m.insert("qualityStats".into(), pair_list(&ge.quality_stats));
    m.insert("baseMods".into(), fmt_list(&ge.base_mods));
    m.insert("levelMods".into(), fmt_list(&ge.level_mods));
    m.insert("qualityMods".into(), Json::Null);
    m.insert(
        "hasGlobalEffect".into(),
        if ge.has_global_effect.get() { Json::Bool(true) } else { Json::Null },
    );
    m.insert(
        "skillTypes".into(),
        set_list(ge.skill_types.as_ref().map(|s| s.iter().map(u32::to_string).collect())),
    );
    m.insert(
        "minionSkillTypes".into(),
        set_list(ge.minion_skill_types.as_ref().map(|s| s.iter().map(u32::to_string).collect())),
    );
    m.insert("addSkillTypes".into(), num_list(&ge.add_skill_types));
    m.insert("excludeSkillTypes".into(), num_list(&ge.exclude_skill_types));
    m.insert("requireSkillTypes".into(), num_list(&ge.require_skill_types));
    m.insert(
        "weaponTypes".into(),
        set_list(ge.weapon_types.as_ref().map(|w| {
            let mut k: Vec<String> = w.keys().cloned().collect();
            k.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            k
        })),
    );
    m.insert("notMinionStat".into(), ge.not_minion_stat.as_ref().map_or(Json::Null, |v| json!(v)));
    m
}

/// dkjson writes an empty Lua table as `[]`.
fn norm_empty(j: &Json) -> Json {
    match j {
        Json::Array(a) if a.is_empty() => Json::Null,
        o => o.clone(),
    }
}

// --- (1) data ----------------------------------------------------------------------------------------------------------------------

#[test]
fn gems_and_lookups_equal_luajit() {
    let f = fixture("gemdata-gems.json");
    let g = gems();
    let sk = skills();
    let want = f["gems"].as_object().unwrap();
    assert_eq!(want.len(), 871);
    assert_eq!(g.gems.len(), want.len(), "gems M");
    let mut bad = Vec::new();
    for (id, w) in want {
        let r = &g.gems[id];
        let mut tags: Vec<&String> = r.tags.keys().collect();
        tags.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        let ge = sk.get(&r.granted_effect_id).expect("grantedEffect");
        let got = json!({
            "id": r.id, "grantedEffect": ge.id, "grantedEffectId": r.granted_effect_id,
            "secondaryGrantedEffect": r.secondary_granted_effect_id.as_ref().and_then(|s| sk.get(s)).map(|e| e.id.clone()),
            "secondaryGrantedEffectId": r.secondary_granted_effect_id,
            "grantedEffectList": if r.secondary_granted_effect_id.is_some() { 2 } else { 1 },
            "naturalMaxLevel": n17(r.natural_max_level), "name": r.name, "gameId": r.game_id, "variantId": r.variant_id,
            "vaalGem": r.vaal_gem, "baseTypeName": r.base_type_name, "tagString": r.tag_string, "tags": tags,
            "secondaryEffectName": r.secondary_effect_name,
            "req": format!("{},{},{}", n17(r.req_str), n17(r.req_dex), n17(r.req_int)),
        });
        let strip = |j: &Json| -> Json {
            Json::Object(
                j.as_object()
                    .unwrap()
                    .iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        };
        if strip(&got) != strip(w) {
            bad.push(format!("{id}: rust {got} lj {w}"));
        }
    }
    assert!(bad.is_empty(), "{} gems differ: {:?}", bad.len(), &bad[..bad.len().min(5)]);
    // gemForSkill (D4) and its collisions: who wins
    let gfs = f["gemForSkill"].as_object().unwrap();
    assert_eq!(f["nGemForSkill"], 822);
    assert_eq!(g.gem_for_skill.len(), gfs.len());
    for (k, v) in gfs {
        assert_eq!(g.gem_for_skill.get(k).map(String::as_str), v.as_str(), "gemForSkill[{k}]");
    }
    let mut by_ge: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, gem) in g.gems.iter() {
        by_ge.entry(&gem.granted_effect_id).or_default().push(id);
    }
    let coll = f["collisions"].as_object().unwrap();
    let ours: Vec<(&&str, &Vec<&str>)> = by_ge.iter().filter(|(_, v)| v.len() > 1).collect();
    assert_eq!(ours.len(), coll.len(), "collisions C");
    assert_eq!(coll.len(), 37);
    for (ge, ids) in ours {
        let mut ids = ids.clone();
        ids.sort_unstable();
        let w = &coll[*ge];
        assert_eq!(json!(ids), w["ids"], "{ge}");
        assert_eq!(g.gem_for_skill.get(*ge).map(String::as_str), w["winner"].as_str(), "{ge} winner");
    }
    let gfbn = f["gemForBaseName"].as_object().unwrap();
    assert_eq!(g.gem_for_base_name.len(), gfbn.len());
    for (k, v) in gfbn {
        assert_eq!(g.gem_for_base_name.get(k).map(String::as_str), v.as_str(), "gemForBaseName[{k}]");
    }
    let gbg = f["gemsByGameId"].as_object().unwrap();
    assert_eq!(g.gems_by_game_id.len(), gbg.len());
    for (gid, w) in gbg {
        let mut l: Vec<String> = g.gems_by_game_id[gid].iter().map(|(v, id)| format!("{v}={id}")).collect();
        l.sort_unstable();
        assert_eq!(json!(l), *w, "gemsByGameId[{gid}]");
    }
    for k in ["gemGrantedEffectIdForVaalGemId", "gemVaalGemIdForBaseGemId"] {
        let ours = if k == "gemGrantedEffectIdForVaalGemId" {
            &g.gem_granted_effect_id_for_vaal_gem_id
        } else {
            &g.gem_vaal_gem_id_for_base_gem_id
        };
        let w = f[k].as_object().unwrap();
        assert_eq!(ours.len(), w.len(), "{k}");
        for (a, b) in w {
            assert_eq!(ours.get(a).map(String::as_str), b.as_str(), "{k}[{a}]");
        }
    }
    let mut ord: Vec<&String> = g.gems.keys().collect();
    ord.sort_unstable();
    assert_eq!(json!(ord), f["orders"]["gems"]);
}

#[test]
fn granted_effects_equal_luajit() {
    let mut want = fixture("gemdata-skills-1.json")["skills"].as_object().unwrap().clone();
    want.extend(fixture("gemdata-skills-2.json")["skills"].as_object().unwrap().clone());
    // a fresh data.skills: the lazy statMap state of one process, as in the probe
    let sk = load_skills_fresh().expect("skills");
    assert_eq!(sk.effects.len(), 1526, "granted effects N");
    assert_eq!(want.len(), sk.effects.len());
    let (mut resolved, mut nil, mut one_of, mut malformed) = (0, 0, 0, 0);
    let mut bad: Vec<String> = Vec::new();
    // resolve every effect's statMap first (the probe resolves in its own `pairs` order; only the shared table's owner matters)
    for ge in &sk.effects {
        let w = &want[&ge.id];
        let empty = Map::new();
        let sm = w["statMap"].as_object().unwrap_or(&empty);
        for (stat, wv) in sm {
            let got = fmt_entry(&sk.stat_map(ge, stat).unwrap_or_else(|e| panic!("{}.{stat}: {e}", ge.id)));
            if got == Json::Bool(false) {
                nil += 1;
            } else {
                resolved += 1;
            }
            if is_one_of(wv) {
                one_of += 1;
            }
            if got.to_string().contains("ERR:") {
                malformed += 1;
            }
            if !matches_lj(wv, &got) {
                bad.push(format!("{}.statMap.{stat}: rust {got} lj {wv}", ge.id));
            }
        }
    }
    for ge in &sk.effects {
        let w = want[&ge.id].as_object().unwrap();
        let lv = sha256_hex(levels_text(ge).as_bytes());
        if w["levels"] != json!(lv) {
            bad.push(format!("{}.levels sha differs", ge.id));
        }
        for (k, got) in effect_record(ge) {
            let wv = w.get(&k).map(norm_empty).unwrap_or(Json::Null);
            let got = norm_empty(&got);
            if !matches_lj(&wv, &got) {
                bad.push(format!("{}.{k}: rust {got} lj {wv}", ge.id));
            }
        }
        for k in w.keys() {
            if !matches!(
                k.as_str(),
                "levels"
                    | "statMap"
                    | "scalars"
                    | "stats"
                    | "constantStats"
                    | "qualityStats"
                    | "baseMods"
                    | "levelMods"
                    | "qualityMods"
                    | "hasGlobalEffect"
                    | "skillTypes"
                    | "minionSkillTypes"
                    | "addSkillTypes"
                    | "excludeSkillTypes"
                    | "requireSkillTypes"
                    | "weaponTypes"
                    | "notMinionStat"
            ) {
                bad.push(format!("{}: unchecked fixture key {k}", ge.id));
            }
        }
    }
    assert!(bad.is_empty(), "{} differences: {:#?}", bad.len(), &bad[..bad.len().min(10)]);
    println!("statMap: resolved K = {resolved}, nil {nil}, lj-strhash $oneOf {one_of}, malformed {malformed}");
    assert_eq!((resolved, nil), (5957, 3782), "statMap resolutions K");
}

/// `#levels` / `next(levels)` come from the LuaJIT patch; sanity: within the stored levels.
#[test]
fn levels_shape_patch_is_consistent() {
    let sk = skills();
    let mut dist: BTreeMap<String, usize> = BTreeMap::new();
    for ge in &sk.effects {
        assert!(
            ge.level(ge.levels_next).is_some(),
            "{}: next(levels) = {} not a level",
            ge.id,
            ge.levels_next
        );
        assert!(ge.levels_len <= ge.levels.len(), "{}", ge.id);
        let seq = ge.levels.iter().enumerate().all(|(i, l)| l.level == (i + 1) as f64);
        let k = format!(
            "{}:#{}",
            if seq { "seq" } else { "sparse" },
            if ge.levels_len == ge.levels.len() {
                "n".into()
            } else {
                ge.levels_len.to_string()
            }
        );
        *dist.entry(k).or_default() += 1;
    }
    println!("levels shapes: {dist:?}");
    // every sequential table has # = n
    assert!(dist.keys().all(|k| !k.starts_with("seq") || k == "seq:#n"), "{dist:?}");
}

/// processMod during a lazy copy sets `hasGlobalEffect` (Data.lua:74-79) -- state of the effect, not of the data file.
#[test]
fn lazy_statmap_copy_sets_has_global_effect() {
    let sk = load_skills_fresh().expect("skills");
    let ssm = skill_stat_map();
    let stat = "blind_effect_+%";
    let e = &ssm.entries[stat];
    assert!(matches!(&e.mods[0], ModElem::Mod(SkillMod::Mod(m)) if m.tags.iter().any(|t| t.ty().is_str("GlobalEffect"))));
    let ge = sk
        .effects
        .iter()
        .find(|g| !g.has_global_effect.get() && g.stat_map_table == g.index && g.id == "Fireball")
        .expect("Fireball");
    assert!(!ge.has_global_effect.get());
    let got = sk.stat_map(ge, stat).unwrap().expect("copied from skillStatMap");
    assert!(ge.has_global_effect.get(), "hasGlobalEffect set by the lazy copy");
    let ModElem::Mod(SkillMod::Mod(m)) = &got.mods[0] else { panic!() };
    assert_eq!(m.source.as_deref(), Some("Skill:Fireball"));
    // R4: the copy is the effect's own -- the global entry keeps no source, a second effect gets its own source
    let ModElem::Mod(SkillMod::Mod(raw)) = &e.mods[0] else { panic!() };
    assert_eq!(raw.source, None);
    let other = sk.get("Arc").unwrap();
    let got2 = sk.stat_map(other, stat).unwrap().unwrap();
    let ModElem::Mod(SkillMod::Mod(m2)) = &got2.mods[0] else { panic!() };
    assert_eq!(m2.source.as_deref(), Some("Skill:Arc"));
    // a second read returns the cached copy
    assert!(std::rc::Rc::ptr_eq(&got, &sk.stat_map(ge, stat).unwrap().unwrap()));
}

/// L24: what LuaJIT does with the malformed mods (format error, lazy-copy error, ModList query error) -- as facts in the fixture.
#[test]
fn malformed_mods_equal_luajit() {
    let f = fixture("gemdata-helpers.json");
    let m = &f["malformed"];
    assert_eq!(m["cannot_be_stunned_while_leeching"]["format"], json!(FORMAT_FLAGS_TABLE_ERROR));
    assert_eq!(m["SupportBloodsoakedBanner"]["format"], json!(FORMAT_FLAGS_TABLE_ERROR));
    assert_eq!(m["spell_damage_+%_per_10_int"]["format"], json!(FORMAT_TAGS_NUMBER_ERROR));
    assert_eq!(m["spell_damage_+%_per_10_int"]["lazy"], json!(PROCESS_MOD_NUMBER_TAG_ERROR));
    assert_eq!(m["cannot_be_stunned_while_leeching"]["lazy"], json!("ok"));
    assert_eq!(m["cannot_be_stunned_while_leeching"]["query"], json!(QUERY_FLAGS_TABLE_ERROR));
    assert_eq!(m["SupportBloodsoakedBanner"]["query"], json!(QUERY_FLAGS_TABLE_ERROR));
    assert_eq!(m["spell_damage_+%_per_10_int"]["query"], json!(QUERY_TAGS_NUMBER_ERROR));
    let ssm = skill_stat_map();
    for (stat, fmt, query) in [
        (
            "cannot_be_stunned_while_leeching",
            FORMAT_FLAGS_TABLE_ERROR,
            QUERY_FLAGS_TABLE_ERROR,
        ),
        ("spell_damage_+%_per_10_int", FORMAT_TAGS_NUMBER_ERROR, QUERY_TAGS_NUMBER_ERROR),
    ] {
        let ModElem::Mod(SkillMod::Malformed(mm)) = &ssm.entries[stat].mods[0] else {
            panic!("{stat} malformed")
        };
        assert_eq!((mm.format_error().as_str(), mm.query_error().as_str()), (fmt, query), "{stat}");
    }
    println!("malformed: {m}");
    let sk = load_skills_fresh().unwrap();
    let fb = sk.get("Fireball").unwrap();
    assert_eq!(
        sk.stat_map(fb, "spell_damage_+%_per_10_int").unwrap_err(),
        PROCESS_MOD_NUMBER_TAG_ERROR
    );
    let ok = sk.stat_map(fb, "cannot_be_stunned_while_leeching").unwrap().unwrap();
    let ModElem::Mod(SkillMod::Malformed(mm)) = &ok.mods[0] else {
        panic!("malformed kept")
    };
    assert_eq!(mm.source.as_deref(), Some("Skill:Fireball"));
    assert_eq!(mm.format_error(), FORMAT_FLAGS_TABLE_ERROR);
    // the type = nil ones are plain inert mods
    let cc = sk.get("SupportCorruptingCry").unwrap();
    let e = sk
        .stat_map(cc, "support_corrupting_cry_warcry_applies_X_stacks_of_corrupted_blood")
        .unwrap()
        .unwrap();
    assert!(matches!(&e.mods[0], ModElem::Mod(SkillMod::Mod(m)) if m.ty.is_none()));
}

// --- (2) helpers -------------------------------------------------------------------------------------------------------------------

#[test]
fn calc_tools_helpers_equal_luajit() {
    let f = fixture("gemdata-helpers.json");
    let sk = skills();
    let s = |j: &Json| j.as_str().map(str::to_owned);
    let mut n = 0;
    let mut bad = Vec::new();
    for c in f["support"].as_array().unwrap() {
        let sup = sk.get(c["s"].as_str().unwrap()).unwrap();
        let act = sk.get(c["a"].as_str().unwrap()).unwrap();
        let empty = Default::default();
        let st = act.skill_types.as_ref().unwrap_or(&empty);
        let (w1, w2) = (s(&c["w1"]), s(&c["w2"]));
        let view = ActiveSkillView {
            granted_effect: act,
            has_gem_data: c["gem"].as_bool().unwrap(),
            src_instance_from_item: c["srcFromItem"].as_bool().unwrap(),
            skill_types: Some(st),
            minion_skill_types: act.minion_skill_types.as_ref(),
            summon: None,
            actor_is_player: !c["notPlayer"].as_bool().unwrap(),
            weapon1: WeaponView {
                ty: w1.as_deref(),
                counts_as_all_1h: c["all1h"].as_bool().unwrap(),
            },
            weapon2: WeaponView {
                ty: w2.as_deref(),
                counts_as_all_1h: false,
            },
        };
        let r = can_granted_effect_support_active_skill(sup, &view, c["imbued"].as_bool().unwrap(), c["granted"].as_bool().unwrap());
        let tri = |l: &Option<Vec<f64>>, minion: bool| match l {
            Some(l) if !l.is_empty() => {
                does_type_expression_match(l, st, if minion { act.minion_skill_types.as_ref() } else { None }).to_string()
            }
            _ => "nil".to_owned(),
        };
        let got = format!(
            "{}|{}|{}",
            r.map_or("ERR".to_owned(), |b| b.to_string()),
            tri(&sup.require_skill_types, true),
            tri(&sup.exclude_skill_types, false)
        );
        n += 3;
        if json!(got) != c["out"] {
            bad.push(format!("{} x {}: rust {got} lj {}", c["a"], c["s"], c["out"]));
        }
    }
    for c in f["req"].as_array().unwrap() {
        let got = get_gem_stat_requirement(c[0].as_f64().unwrap(), c[1].as_bool().unwrap(), c[2].as_f64().unwrap());
        n += 1;
        if json!(n17(got)) != c[3] {
            bad.push(format!("req {c}: rust {got}"));
        }
    }
    let mut errs = 0;
    for c in f["stats"].as_array().unwrap() {
        let ge = sk.get(c["id"].as_str().unwrap()).unwrap();
        let si = SkillInstance {
            level: c["level"].as_f64().unwrap(),
            quality: c["quality"].as_f64().unwrap(),
            actor_level: c["actorLevel"].as_f64(),
        };
        n += 1;
        match build_skill_instance_stats(&si, ge) {
            Ok(st) => {
                let mut kv: Vec<(&String, &f64)> = st.iter().collect();
                kv.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                let text = kv.iter().map(|(k, v)| format!("{k}={}", n17(**v))).collect::<Vec<_>>().join(";");
                if json!(sha256_hex(text.as_bytes())) != c["sha"] {
                    bad.push(format!("stats {c}: rust {text}"));
                }
            }
            Err(e) => {
                errs += 1;
                if c["n"] != -1 {
                    bad.push(format!("stats {c}: rust error {e}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} differences: {:#?}", bad.len(), &bad[..bad.len().min(10)]);
    println!("helpers: {n} cases = LuaJIT ({errs} Lua errors)");
    assert!(n >= 500);
}

#[test]
fn process_gem_level_equal_luajit() {
    let f = fixture("gemdata-helpers.json");
    let sk = skills();
    let modes = ["normalMaximum", "corruptedMaximum", "awakenedMaximum", "levelOne"];
    let mut n = 0;
    for (id, want) in f["gemLevel"].as_object().unwrap() {
        let gem = &gems().gems[id];
        let r = |v: Result<f64, String>| v.map_or("ERR".to_owned(), n17);
        let mut out: Vec<String> = modes.iter().map(|m| r(process_gem_level(gem, false, m, Some(1.0), &sk))).collect();
        for cl in [1.0, 12.0, 38.0, 70.0, 100.0] {
            out.push(r(process_gem_level(gem, false, "characterLevel", Some(cl), &sk)));
        }
        out.push(r(process_gem_level(gem, true, "normalMaximum", None, &sk)));
        n += out.len();
        assert_eq!(json!(out.join(",")), *want, "{id}");
    }
    println!("ProcessGemLevel: {n} cases = LuaJIT");
}

#[test]
fn find_skill_gem_equal_luajit() {
    let f = fixture("gemdata-helpers.json");
    let (mut n, mut amb) = (0, 0);
    for c in f["find"].as_array().unwrap() {
        let spec = c["spec"].as_str().unwrap();
        let (err, gem) = find_skill_gem(spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
        n += 1;
        assert_eq!(gem.map(|g| g.id.as_str()), c["gem"].as_str(), "{spec}");
        if c["err"] == "Ambiguous" {
            // R13: LuaJIT names two of the first-level matches in its per-process `pairs` order
            amb += 1;
            let err = err.unwrap_or_else(|| panic!("{spec}: no error"));
            let m: Vec<&str> = c["matches"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
            let prefix = format!("Ambiguous gem name '{spec}': matches '");
            let rest = err
                .strip_prefix(&prefix)
                .and_then(|r| r.strip_suffix('\''))
                .unwrap_or_else(|| panic!("{err}"));
            let (a, b) = rest.split_once("', '").unwrap_or_else(|| panic!("{err}"));
            assert!(a != b && m.contains(&a) && m.contains(&b), "{spec}: {err} vs {m:?}");
        } else {
            assert_eq!(err.as_deref(), c["err"].as_str(), "{spec}");
        }
    }
    println!("FindSkillGem: {n} nameSpecs = LuaJIT ({amb} ambiguous, lj-strhash)");
}

// --- (3) goldens --------------------------------------------------------------------------------------------------------------------

fn gem_dump(g: &GemInstance) -> Json {
    let mut m = Map::new();
    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            m.insert(k.to_owned(), Json::String(v));
        }
    };
    let num = |v: Option<f64>| v.map(n17);
    let b = |v: bool| Some(v.to_string());
    put("nameSpec", g.name_spec.clone());
    put("gemId", g.gem_id.clone());
    put("skillId", g.skill_id.clone());
    put("level", num(g.level));
    put("quality", num(g.quality));
    put("enabled", b(g.enabled));
    put("enableGlobal1", g.enable_global1.map(|v| v.to_string()));
    put("enableGlobal2", g.enable_global2.map(|v| v.to_string()));
    put("count", num(g.count));
    put("skillPart", num(g.skill_part));
    put("skillPartCalcs", num(g.skill_part_calcs));
    put("skillStageCount", num(g.skill_stage_count));
    put("skillStageCountCalcs", num(g.skill_stage_count_calcs));
    put("skillMineCount", num(g.skill_mine_count));
    put("skillMineCountCalcs", num(g.skill_mine_count_calcs));
    put("skillMinion", g.skill_minion.clone());
    put("skillMinionCalcs", g.skill_minion_calcs.clone());
    put("skillMinionItemSet", num(g.skill_minion_item_set));
    put("skillMinionItemSetCalcs", num(g.skill_minion_item_set_calcs));
    put("skillMinionSkill", num(g.skill_minion_skill));
    put("skillMinionSkillCalcs", num(g.skill_minion_skill_calcs));
    put("color", g.color.clone());
    put("errMsg", g.err_msg.clone());
    put("gemData", g.gem_data.clone());
    put("grantedEffect", g.granted_effect.clone());
    put("reqLevel", num(g.req_level));
    put("reqStr", num(g.req_str));
    put("reqDex", num(g.req_dex));
    put("reqInt", num(g.req_int));
    put("naturalMaxLevel", num(g.natural_max_level));
    put("new", g.new.map(|v| v.to_string()));
    let mut keys: Vec<String> = m.keys().cloned().collect();
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    m.insert("keys".into(), json!(keys));
    Json::Object(m)
}

fn tab_dump(t: &SkillsTab, character_level: f64) -> Json {
    let sets: Vec<Json> = t
        .skill_sets
        .iter()
        .map(|s| {
            let groups: Vec<Json> = s
                .socket_group_list
                .iter()
                .map(|g| {
                    let mut m = Map::new();
                    let mut put = |k: &str, v: Option<String>| {
                        if let Some(v) = v {
                            m.insert(k.to_owned(), Json::String(v));
                        }
                    };
                    put("enabled", Some(g.enabled.to_string()));
                    put("includeInFullDPS", g.include_in_full_dps.map(|v| v.to_string()));
                    put("groupCount", g.group_count.map(n17));
                    put("label", g.label.clone());
                    put("slot", g.slot.clone());
                    put("source", g.source.clone());
                    put("mainActiveSkill", g.main_active_skill.map(n17));
                    put("mainActiveSkillCalcs", g.main_active_skill_calcs.map(n17));
                    put("imbuedSupport", g.imbued_support.clone());
                    let mut keys: Vec<String> = m.keys().cloned().chain(["gemList".to_owned()]).collect();
                    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
                    m.insert("keys".into(), json!(keys));
                    m.insert("gems".into(), json!(g.gem_list.iter().map(gem_dump).collect::<Vec<_>>()));
                    Json::Object(m)
                })
                .collect();
            let mut o = json!({ "id": n17(s.id), "groups": groups });
            if let Some(t) = &s.title {
                o["title"] = json!(t);
            }
            o
        })
        .collect();
    let imbued: Map<String, Json> = t.imbued_support_by_slot.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    json!({
        "sets": sets, "activeSkillSetId": n17(t.active_skill_set_id), "active": t.socket_group_list().len(),
        "imbued": if imbued.is_empty() { json!([]) } else { Json::Object(imbued) },
        "defaultGemLevel": t.default_gem_level, "defaultGemQuality": n17(t.default_gem_quality), "characterLevel": n17(character_level),
    })
}

/// `want` may hold `$oneOf` at the gem level (R13).
fn compare(want: &Json, got: &Json, path: &str, bad: &mut Vec<String>) {
    if is_one_of(want) {
        if !matches_lj(want, got) {
            bad.push(format!("{path}: rust {got} not in lj {want}"));
        }
        return;
    }
    match (want, got) {
        (Json::Object(w), Json::Object(g)) => {
            for k in w.keys().chain(g.keys().filter(|k| !w.contains_key(*k))) {
                compare(
                    w.get(k).unwrap_or(&Json::Null),
                    g.get(k).unwrap_or(&Json::Null),
                    &format!("{path}.{k}"),
                    bad,
                );
            }
        }
        (Json::Array(w), Json::Array(g)) if w.len() == g.len() => {
            for (i, (a, b)) in w.iter().zip(g).enumerate() {
                compare(a, b, &format!("{path}.{i}"), bad);
            }
        }
        _ if want == got => {}
        _ => bad.push(format!("{path}: rust {got} lj {want}")),
    }
}

#[test]
fn golden_socket_groups_equal_luajit() {
    let f = fixture("gemdata-groups.json");
    let sk = skills();
    let mut bad = Vec::new();
    let mut summary = Vec::new();
    for g in GOLDEN_BUILDS {
        let model = pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
        let tab = load_skills_tab(&model.skills, &sk).unwrap_or_else(|e| panic!("{}: {e}", g.name));
        let got = tab_dump(&tab, model.level);
        compare(&f["builds"][g.name], &got, g.name, &mut bad);
        let groups: usize = tab.skill_sets.iter().map(|s| s.socket_group_list.len()).sum();
        let gems_n: usize = tab
            .skill_sets
            .iter()
            .flat_map(|s| &s.socket_group_list)
            .map(|sg| sg.gem_list.len())
            .sum();
        summary.push(format!("{}: {groups} groups / {gems_n} gems", g.name));
    }
    assert!(bad.is_empty(), "{} differences: {:#?}", bad.len(), &bad[..bad.len().min(10)]);
    println!("{summary:#?}");
}

/// Cold / warm load of `skills.json` (+ the stat map) -- the perf line of the report.
#[test]
fn skills_load_time() {
    let t = std::time::Instant::now();
    let s: Skills = load_skills_fresh().unwrap();
    let cold = t.elapsed();
    let t = std::time::Instant::now();
    let s2: Skills = load_skills_fresh().unwrap();
    let warm = t.elapsed();
    let t = std::time::Instant::now();
    let _ = skill_stat_map();
    let ssm = t.elapsed();
    assert_eq!(s.effects.len(), s2.effects.len());
    println!("skills.json load: first {cold:?}, second {warm:?}; skill-stat-map.json {ssm:?}");
}
