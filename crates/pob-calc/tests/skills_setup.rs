//! R6.3 gate: the skill half of `calcs.initEnv` (CalcSetup.lua :1308-1356, :1448-1940, CalcActiveSkill.lua `createActiveSkill`) against the
//! LuaJIT fixtures of R6.1 (`tests/fixtures/<build>.skills.json`, producer `tools/lua-corpus/export-skills.ts`), 6 goldens,
//! `ParseMode::Full`, skills on.
//!
//!   * `socketGroups` -- every field the probe dumps (D15/R13: a gem with several candidate variants is masked by the probe; Rust's id must
//!     be one of `ambiguousVariants`);
//!   * every active skill: `activeEffect`, `effectList` (order), `supportList` (order), `skillTypes`, `minionSkillTypes`, `socketGroup`,
//!     `slotName`, `summonSkill`, `disableReason`; `skillFlags` -- the flags createActiveSkill sets must all be in LuaJIT's;
//!   * `mainSkill`, `requirementsTableGems`, `env.modDB` after initEnv (mods as a multiset per name, conditions, multipliers).
//!
//! What LuaJIT's dump holds that this phase cannot have yet is listed per key, not hidden: the fields `buildActiveSkillModList` (R6.4,
//! CalcActiveSkill.lua:229-877) writes -- onto the skill, its effects and the gem instances -- and `displaySkillListCalcs` (the CALCS-mode
//! initEnv of CalcsTab, not MAIN). Each such key is counted and printed; a difference on any other key fails.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pob_calc::active_skill::{EffectRef, GemCfg};
use pob_calc::skills::SkillsState;
use pob_calc::skills_tab::{ExplodeSource, GemInstance, SocketGroup};
use pob_calc::{Env, InitEnvOptions, ParseMode, init_env_with};
use pob_data::gems::gems;
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::format::format_source_mod;
use pob_mods::{StoreKind, Value};
use serde_json::{Map, Value as Json, json};

const D15: &str = "<D15:ambiguous>";

/// `buildActiveSkillModList` (R6.4) writes these onto the active skill (CalcActiveSkill.lua:229-877, :1941-1970).
const R64_SKILL_KEYS: [&str; 17] = [
    "actor",
    "baseSame",
    "baseSkillModList",
    "buffList",
    "extraSkillModList",
    "minion",
    "minionList",
    "skillCfg",
    "skillData",
    "skillModList",
    "skillPart",
    "skillPartName",
    "skillTotemId",
    "skillTypesIsCfgTypes",
    "triggeredBy",
    "weapon1Cfg",
    "weapon2Cfg",
];
/// ... and these (computed by R6.4 but dumped at the skill level).
const R64_SKILL_KEYS2: [&str; 2] = ["weapon1Flags", "weapon2Flags"];
/// ... onto the effects (`grantedEffectLevel` :514/:579, `supportQuality` :564, `actorLevel` :577).
const R64_EFFECT_KEYS: [&str; 3] = ["grantedEffectLevel", "actorLevel", "supportQuality"];
/// ... onto `activeEffect.srcInstance` (:251-269, :629-630, :656-657, :697-743).
const R64_GEM_KEYS: [&str; 12] = [
    "skillPart",
    "skillPartCalcs",
    "skillMineCount",
    "skillMineCountCalcs",
    "skillStageCount",
    "skillStageCountCalcs",
    "skillMinion",
    "skillMinionCalcs",
    "skillMinionItemSet",
    "skillMinionItemSetCalcs",
    "skillMinionSkill",
    "skillMinionSkillCalcs",
];

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.skills.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn n17(x: f64) -> Json {
    Json::String(format_g(x, 17))
}

fn run(name: &str) -> Env {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let model = pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("xml parses");
    init_env_with(
        Rc::new(model),
        InitEnvOptions {
            parse_mode: ParseMode::Full,
            ..InitEnvOptions::default()
        },
        true,
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn gref((g, i): (usize, usize)) -> Json {
    json!(format!("{}:{}", g + 1, i + 1))
}

struct Ctx<'a> {
    st: &'a SkillsState,
}

impl Ctx<'_> {
    fn effect_id(&self, i: usize) -> &str {
        &self.st.skills.effects[i].id
    }

    /// `effBrief` of the probe.
    fn brief(&self, e: &EffectRef) -> Json {
        let e = e.borrow();
        let mut o = Map::new();
        o.insert("grantedEffect".into(), json!(self.effect_id(e.granted_effect)));
        if let Some(l) = e.level {
            o.insert("level".into(), n17(l));
        }
        if let Some(q) = e.quality {
            o.insert("quality".into(), n17(q));
        }
        if let Some(s) = e.src_instance {
            o.insert("srcInstance".into(), gref(s));
        }
        Json::Object(o)
    }

    fn gem_cfg(c: &GemCfg) -> Json {
        let mut o = Map::new();
        if let Some(s) = &c.group.slot_name {
            o.insert("slotName".into(), json!(s));
        }
        o.insert(
            "propertyModList".into(),
            json!(
                c.group
                    .property_mod_list
                    .iter()
                    .map(|e| format_source_mod(&e.mod_))
                    .collect::<Vec<_>>()
            ),
        );
        for (k, v) in [
            ("intelligenceGems", c.group.intelligence_gems),
            ("dexterityGems", c.group.dexterity_gems),
            ("strengthGems", c.group.strength_gems),
        ] {
            if let Some(v) = v {
                o.insert(k.into(), n17(v));
            }
        }
        if let Some(s) = &c.socket_color {
            o.insert("socketColor".into(), json!(s));
        }
        o.insert("socketNum".into(), n17(c.socket_num));
        Json::Object(o)
    }

    /// `fields(effect)` of the probe.
    fn effect(&self, e: &EffectRef) -> Json {
        let e = e.borrow();
        let mut o = Map::new();
        o.insert("grantedEffect".into(), json!(self.effect_id(e.granted_effect)));
        let mut num = |k: &str, v: Option<f64>| {
            if let Some(v) = v {
                o.insert(k.into(), n17(v));
            }
        };
        num("level", e.level);
        num("quality", e.quality);
        num("globalQuality", e.global_quality);
        num("itemQuality", e.item_quality);
        num("supportQuality", e.support_quality);
        num("socketQuality", e.socket_quality);
        num("req", e.req);
        for (k, v) in &e.other {
            o.insert(k.clone(), n17(*v));
        }
        if let Some(s) = e.src_instance {
            o.insert("srcInstance".into(), gref(s));
        }
        if let Some(g) = &e.gem_data {
            o.insert("gemData".into(), json!(g));
        }
        if let Some(b) = e.superseded {
            o.insert("superseded".into(), json!(b));
        }
        if let Some(b) = e.enabled {
            o.insert("enabled".into(), json!(b));
        }
        if let Some(b) = e.applies_to_granted_skills {
            o.insert("appliesToGrantedSkills".into(), json!(b));
        }
        if let Some(set) = &e.is_supporting {
            let mut v: Vec<String> = set.iter().map(|&s| gref(s).as_str().unwrap().to_owned()).collect();
            v.sort();
            o.insert("isSupporting".into(), json!(v));
        }
        if let Some(c) = &e.gem_cfg {
            o.insert("gemCfg".into(), Self::gem_cfg(c));
        }
        if let Some(info) = &e.gem_property_info {
            let mut m = Map::new();
            for (i, t) in info.iter().enumerate() {
                m.insert(
                    (i + 1).to_string(),
                    json!({ "mod": format_source_mod(&t.mod_), "value": lua_value(&t.value) }),
                );
            }
            o.insert("gemPropertyInfo".into(), Json::Object(m));
        }
        Json::Object(o)
    }

    fn gem(&self, gi: &GemInstance) -> Json {
        let mut o = Map::new();
        let mut s = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                o.insert(k.into(), json!(v));
            }
        };
        s("nameSpec", &gi.name_spec);
        s("gemId", &gi.gem_id);
        s("skillId", &gi.skill_id);
        s("skillMinion", &gi.skill_minion);
        s("skillMinionCalcs", &gi.skill_minion_calcs);
        s("color", &gi.color);
        s("errMsg", &gi.err_msg);
        s("gemData", &gi.gem_data);
        s("grantedEffect", &gi.granted_effect);
        let mut num = |k: &str, v: Option<f64>| {
            if let Some(v) = v {
                o.insert(k.into(), n17(v));
            }
        };
        num("level", gi.level);
        num("quality", gi.quality);
        num("count", gi.count);
        num("skillPart", gi.skill_part);
        num("skillPartCalcs", gi.skill_part_calcs);
        num("skillStageCount", gi.skill_stage_count);
        num("skillStageCountCalcs", gi.skill_stage_count_calcs);
        num("skillMineCount", gi.skill_mine_count);
        num("skillMineCountCalcs", gi.skill_mine_count_calcs);
        num("skillMinionItemSet", gi.skill_minion_item_set);
        num("skillMinionItemSetCalcs", gi.skill_minion_item_set_calcs);
        num("skillMinionSkill", gi.skill_minion_skill);
        num("skillMinionSkillCalcs", gi.skill_minion_skill_calcs);
        num("reqLevel", gi.req_level);
        num("reqStr", gi.req_str);
        num("reqDex", gi.req_dex);
        num("reqInt", gi.req_int);
        num("naturalMaxLevel", gi.natural_max_level);
        num("triggerChance", gi.trigger_chance);
        num("reqOverride", gi.req_override);
        let mut b = |k: &str, v: Option<bool>| {
            if let Some(v) = v {
                o.insert(k.into(), json!(v));
            }
        };
        b("enabled", Some(gi.enabled));
        b("enableGlobal1", gi.enable_global1);
        b("enableGlobal2", gi.enable_global2);
        b("new", gi.new);
        b("matchesSocket", gi.matches_socket);
        b("fromItem", gi.from_item);
        b("noSupports", gi.no_supports);
        b("triggered", gi.triggered.then_some(true));
        if let Some(e) = &gi.display_effect {
            o.insert("displayEffect".into(), self.brief(e));
        }
        if let Some(e) = &gi.support_effect {
            o.insert("supportEffect".into(), self.brief(e));
        }
        if let Some(src) = &gi.explode_source {
            o.insert("explodeSource".into(), explode(src));
        }
        if let Some(gd) = &gi.gem_data {
            // the probe's `ambiguousVariants`: the variants of the gameId with the same grantedEffectId
            let data = gems();
            let g = &data.gems[gd];
            if let Some(vs) = data.gems_by_game_id.get(&g.game_id) {
                let mut c: Vec<String> = vs
                    .values()
                    .filter(|id| Some(&data.gems[id.as_str()].granted_effect_id) == gi.skill_id.as_ref())
                    .cloned()
                    .collect();
                c.sort();
                if c.len() > 1 {
                    o.insert("ambiguousVariants".into(), json!(c));
                }
            }
        }
        Json::Object(o)
    }

    fn group(&self, sg: &SocketGroup) -> Json {
        let mut o = Map::new();
        o.insert("enabled".into(), json!(sg.enabled));
        let mut s = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                o.insert(k.into(), json!(v));
            }
        };
        s("label", &sg.label);
        s("slot", &sg.slot);
        s("source", &sg.source);
        s("imbuedSupport", &sg.imbued_support);
        s("displayLabel", &sg.display_label);
        let mut num = |k: &str, v: Option<f64>| {
            if let Some(v) = v {
                o.insert(k.into(), n17(v));
            }
        };
        num("groupCount", sg.group_count);
        num("mainActiveSkill", sg.main_active_skill);
        num("mainActiveSkillCalcs", sg.main_active_skill_calcs);
        num("sourceNode", sg.source_node.map(f64::from));
        if let Some(b) = sg.include_in_full_dps {
            o.insert("includeInFullDPS".into(), json!(b));
        }
        if let Some(b) = sg.no_supports {
            o.insert("noSupports".into(), json!(b));
        }
        if let Some(b) = sg.slot_enabled {
            o.insert("slotEnabled".into(), json!(b));
        }
        if let Some(i) = &sg.source_item {
            o.insert("sourceItem".into(), json!(i.0.name));
        }
        if let Some(l) = &sg.display_skill_list {
            o.insert(
                "displaySkillList".into(),
                json!(l.iter().map(|&i| n17((i + 1) as f64)).collect::<Vec<_>>()),
            );
        }
        if let Some(l) = &sg.display_gem_list {
            o.insert("displayGemList".into(), json!(l.iter().map(|&g| gref(g)).collect::<Vec<_>>()));
        }
        if let Some(l) = &sg.explode_sources {
            o.insert("explodeSources".into(), json!(l.iter().map(explode).collect::<Vec<_>>()));
        }
        o.insert("gemList".into(), json!(sg.gem_list.iter().map(|g| self.gem(g)).collect::<Vec<_>>()));
        Json::Object(o)
    }
}

/// An explode source by identity: the node id (the probe dumps the node's OWN scalar fields -- a spec node keeps `id` behind
/// `__index`, so only a generated cluster node shows it) or the item's name.
fn explode(src: &ExplodeSource) -> Json {
    match src {
        ExplodeSource::Node(id) => json!({ "$node": id }),
        ExplodeSource::Item(i) => json!({ "$item": i.0.name }),
    }
}

/// `D(v)` of a value table (numbers `%.17g`, tables keyed by `tostring(k)`).
fn lua_value(v: &Value) -> Json {
    match v {
        Value::Nil => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Num(x) => n17(*x),
        Value::Str(s) => json!(s.to_string()),
        Value::Table(t) => {
            let mut m = Map::new();
            for (i, x) in t.array.iter().enumerate() {
                m.insert((i + 1).to_string(), lua_value(x));
            }
            for (k, x) in &t.hash {
                m.insert(k.to_string(), lua_value(x));
            }
            Json::Object(m)
        }
        Value::Mod(m) => json!(format_source_mod(m)),
        Value::Func(_) => json!("<function>"),
    }
}

fn skill_types(s: &BTreeSet<u32>) -> Json {
    Json::Object(s.iter().map(|t| (t.to_string(), json!(true))).collect())
}

/// Compare `want` (LuaJIT) with `got` (Rust) under `path`, recording every difference as `(path, lj, rust)`.
fn compare(want: &Json, got: &Json, path: &str, out: &mut Vec<(String, String)>) {
    match (want, got) {
        (Json::Object(w), Json::Object(g)) => {
            let keys: BTreeSet<&String> = w.keys().chain(g.keys()).collect();
            for k in keys {
                compare(
                    w.get(k).unwrap_or(&Json::Null),
                    g.get(k).unwrap_or(&Json::Null),
                    &format!("{path}.{k}"),
                    out,
                );
            }
        }
        (Json::Array(w), Json::Array(g)) if w.len() == g.len() => {
            for (i, (a, b)) in w.iter().zip(g).enumerate() {
                compare(a, b, &format!("{path}[{i}]"), out);
            }
        }
        _ if want == got => {}
        _ => out.push((path.to_owned(), format!("lj {want} rust {got}"))),
    }
}

/// The key a difference is classified by: the last `.name` of the path.
fn last_key(path: &str) -> &str {
    let p = path.rsplit('.').next().unwrap_or(path);
    p.split('[').next().unwrap_or(p)
}

/// Apply the probe's D15 mask to the Rust dump: an ambiguous gem id must be one of LuaJIT's candidates, then becomes the sentinel.
fn mask(v: &mut Json, amb: &BTreeSet<String>, bad: &mut Vec<String>) {
    match v {
        Json::String(s) if amb.contains(s.as_str()) => *s = D15.into(),
        Json::Array(a) => a.iter_mut().for_each(|x| mask(x, amb, bad)),
        Json::Object(o) => {
            if let Some(Json::Array(c)) = o.get("ambiguousVariants").cloned() {
                let gd = o.get("gemData").and_then(Json::as_str).unwrap_or("").to_owned();
                if !c.iter().any(|x| x.as_str() == Some(gd.as_str())) {
                    bad.push(format!("D15: rust gemData {gd} not in {c:?}"));
                }
                o.insert("nameSpec".into(), json!(D15));
            }
            for (k, x) in o.iter_mut() {
                if k != "ambiguousVariants" {
                    mask(x, amb, bad);
                }
            }
        }
        _ => {}
    }
}

fn db_dump(env: &Env) -> Json {
    let st = env.mod_db.borrow();
    let StoreKind::Db(db) = &st.kind else { panic!("not a ModDB") };
    let mut mods = Map::new();
    let mut count = 0;
    for (name, bucket) in &db.mods {
        let mut v: Vec<String> = bucket.iter().map(|m| format_source_mod(m)).collect();
        v.sort();
        count += v.len();
        mods.insert(name.to_string(), json!(v));
    }
    let conditions: Map<String, Json> = st.conditions.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let multipliers: Map<String, Json> = st.multipliers.iter().map(|(k, v)| (k.to_string(), n17(*v))).collect();
    json!({ "mods": mods, "count": format_g(count as f64, 17), "conditions": conditions, "multipliers": multipliers })
}

/// One golden: (differences that fail, R6.4/CALCS keys counted, summary).
fn check(name: &str) -> (Vec<String>, BTreeMap<String, usize>, String) {
    let lj = fixture(name);
    let env = run(name);
    let st = env.skills.as_ref().expect("skills on");
    let cx = Ctx { st };
    let groups = st.tab.socket_group_list();
    let mut bad = Vec::new();
    let mut later: BTreeMap<String, usize> = BTreeMap::new();

    let amb: BTreeSet<String> = lj["socketGroups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["gemList"].as_array().unwrap().iter())
        .flat_map(|gem| gem["ambiguousVariants"].as_array().cloned().unwrap_or_default())
        .map(|x| x.as_str().unwrap().to_owned())
        .collect();

    // --- socketGroups ---------------------------------------------------------------------------------------------------
    let mut got_groups = json!(groups.iter().map(|g| cx.group(g)).collect::<Vec<_>>());
    mask(&mut got_groups, &amb, &mut bad);
    let want_groups = &lj["socketGroups"];
    if want_groups.as_array().unwrap().len() != groups.len() {
        bad.push(format!(
            "socketGroups: lj {} rust {}",
            want_groups.as_array().unwrap().len(),
            groups.len()
        ));
    }
    let mut diffs = Vec::new();
    // the explode sources: by identity (see `explode`)
    let mut want_groups = want_groups.clone();
    for (gi, g) in want_groups.as_array_mut().unwrap().iter_mut().enumerate() {
        let rust_sources: Vec<Json> = groups
            .get(gi)
            .and_then(|sg| sg.explode_sources.clone())
            .unwrap_or_default()
            .iter()
            .map(explode)
            .collect();
        if let Some(list) = g.get_mut("explodeSources").and_then(Json::as_array_mut) {
            for (k, src) in list.iter_mut().enumerate() {
                *src = explode_identity(src, rust_sources.get(k));
            }
        }
        for (k, gem) in g["gemList"].as_array_mut().unwrap().iter_mut().enumerate() {
            if let Some(src) = gem.get_mut("explodeSource") {
                let r = groups
                    .get(gi)
                    .and_then(|sg| sg.gem_list.get(k))
                    .and_then(|x| x.explode_source.as_ref())
                    .map(explode);
                *src = explode_identity(src, r.as_ref());
            }
        }
    }
    // R6.4 :558-567 adds `SupportedGemProperty` values onto `activeEffect[key]` and a `GemSupport<Key>` BASE mod per value into the
    // skill's skillModList: LuaJIT's level/quality of an ACTIVE effect = Rust's + the sum of those mods (checked to the number).
    let lj_skills = lj["activeSkills"].as_array().unwrap();
    let support_delta = |i: usize, key: &str| -> f64 {
        let name = if key == "level" { "GemSupportLevel" } else { "GemSupportQuality" };
        lj_skills[i]["skillModList"]["mods"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| {
                let parts: Vec<&str> = m.as_str().unwrap().split('|').collect();
                (parts.get(2) == Some(&name)).then(|| parts[0].parse::<f64>().expect("number"))
            })
            .sum()
    };
    let skill_of_gem: BTreeMap<String, usize> = lj_skills
        .iter()
        .enumerate()
        .filter_map(|(i, sk)| sk["activeEffect"]["srcInstance"].as_str().map(|g| (g.to_owned(), i)))
        .collect();
    let num_of = |v: Option<&Json>| v.and_then(Json::as_str).and_then(|x| x.parse::<f64>().ok());
    let r64_level =
        |skill: Option<usize>, key: &str, lj_v: Option<f64>, rust_v: Option<f64>, later: &mut BTreeMap<String, usize>| -> bool {
            match (skill, lj_v, rust_v) {
                (Some(i), Some(a), Some(b)) if (key == "level" || key == "quality") && a - b == support_delta(i, key) => {
                    *later
                        .entry(format!(
                            "effect.{key} of an active effect (R6.4 :558-567 SupportedGemProperty = sum of GemSupport* mods)"
                        ))
                        .or_default() += 1;
                    true
                }
                _ => false,
            }
        };
    let pointer = |root: &Json, p: &str| -> Option<Json> {
        // "socketGroups[1].gemList[0].displayEffect.level" -> JSON pointer
        let mut ptr = String::new();
        for seg in p.split('.') {
            let (name, idx) = match seg.find('[') {
                Some(b) => (&seg[..b], Some(&seg[b + 1..seg.len() - 1])),
                None => (seg, None),
            };
            ptr.push('/');
            ptr.push_str(name);
            if let Some(i) = idx {
                ptr.push('/');
                ptr.push_str(i);
            }
        }
        root.pointer(&ptr).cloned()
    };
    let processed_in_main = |gi: usize| -> bool {
        let sg = &groups[gi];
        (gi + 1) as f64 == env.main_socket_group || (sg.enabled && sg.slot_enabled == Some(true))
    };
    compare(&want_groups, &got_groups, "socketGroups", &mut diffs);
    let roots = (json!({ "socketGroups": want_groups }), json!({ "socketGroups": got_groups }));
    for (p, d) in diffs {
        let k = last_key(&p);
        let group_idx: usize = p["socketGroups[".len()..].split(']').next().unwrap().parse().unwrap();
        if k == "displaySkillListCalcs" {
            *later.entry("group.displaySkillListCalcs (CALCS-mode initEnv)".into()).or_default() += 1;
        } else if k == "displayGemList" && !processed_in_main(group_idx) {
            // set only by the processing loop (:1792, :1857); a group MAIN does not process keeps the CALCS-mode run's value
            *later
                .entry("group.displayGemList of a group MAIN does not process (CALCS-mode leftover)".into())
                .or_default() += 1;
        } else if p.contains(".displayEffect.")
            && r64_level(
                pointer(&roots.0, &p.replace(&format!(".displayEffect.{k}"), ".displayEffect.srcInstance"))
                    .and_then(|g| g.as_str().and_then(|g| skill_of_gem.get(g).copied())),
                k,
                num_of(pointer(&roots.0, &p).as_ref()),
                num_of(pointer(&roots.1, &p).as_ref()),
                &mut later,
            )
        {
        } else if R64_GEM_KEYS.contains(&k)
            && p.split(".gemList[")
                .nth(1)
                .and_then(|r| r.split(']').next())
                .and_then(|g| g.parse::<usize>().ok())
                .is_some_and(|gem| skill_of_gem.contains_key(&format!("{}:{}", group_idx + 1, gem + 1)))
        {
            // only on a gem that is some active skill's `srcInstance`
            *later.entry(format!("gem.{k} (R6.4 srcInstance write)")).or_default() += 1;
        } else {
            bad.push(format!("{p}: {d}"));
        }
    }

    // --- activeSkills ---------------------------------------------------------------------------------------------------
    let want_skills = lj["activeSkills"].as_array().unwrap();
    if want_skills.len() != st.active_skill_list.len() {
        bad.push(format!(
            "activeSkills: lj {} rust {}",
            want_skills.len(),
            st.active_skill_list.len()
        ));
    }
    let mut flags_missing = 0;
    for (i, (w, s)) in want_skills.iter().zip(&st.active_skill_list).enumerate() {
        let mut o = Map::new();
        o.insert("index".into(), n17((i + 1) as f64));
        o.insert("activeEffect".into(), cx.effect(&s.active_effect));
        o.insert(
            "effectList".into(),
            json!(s.effect_list.iter().map(|e| cx.effect(e)).collect::<Vec<_>>()),
        );
        o.insert(
            "supportList".into(),
            json!(s.support_list.iter().map(|e| cx.effect(e)).collect::<Vec<_>>()),
        );
        o.insert("skillTypes".into(), skill_types(&s.skill_types));
        if let Some(m) = &s.minion_skill_types {
            o.insert("minionSkillTypes".into(), skill_types(m));
        }
        if let Some(g) = s.socket_group {
            o.insert("socketGroup".into(), n17((g + 1) as f64));
        }
        if let Some(sl) = &s.slot_name {
            o.insert("slotName".into(), json!(sl));
        }
        let mut got = Json::Object(o);
        mask(&mut got, &amb, &mut bad);
        let mut want = w.clone();
        let wo = want.as_object_mut().unwrap();
        // skillFlags: createActiveSkill's flags must all be LuaJIT's; the rest is R6.4
        let lj_flags: BTreeSet<String> = wo
            .remove("skillFlags")
            .and_then(|f| f.as_object().cloned())
            .map(|f| f.into_iter().filter(|(_, v)| v == &json!(true)).map(|(k, _)| k).collect())
            .unwrap_or_default();
        for (f, v) in &s.skill_flags {
            // the probe's `D` skips every key named `minion` (SKIP), `skillFlags.minion` included
            if *v && !lj_flags.contains(f) && f != "minion" {
                bad.push(format!("activeSkills[{i}].skillFlags.{f}: rust true, lj not set"));
            }
        }
        flags_missing += lj_flags.iter().filter(|f| !s.skill_flags.contains_key(*f)).count();
        for k in R64_SKILL_KEYS.iter().chain(&R64_SKILL_KEYS2) {
            if wo.remove(*k).is_some() {
                *later.entry(format!("skill.{k} (R6.4)")).or_default() += 1;
            }
        }
        let mut diffs = Vec::new();
        compare(&want, &got, &format!("activeSkills[{i}]"), &mut diffs);
        for (p, d) in diffs {
            let k = last_key(&p);
            let on_active = p == format!("activeSkills[{i}].activeEffect.{k}") || p == format!("activeSkills[{i}].effectList[0].{k}");
            let rel = p.split_once('.').map_or("", |x| x.1);
            if on_active
                && r64_level(
                    Some(i),
                    k,
                    num_of(pointer(&want, rel).as_ref()),
                    num_of(pointer(&got, rel).as_ref()),
                    &mut later,
                )
            {
            } else if R64_EFFECT_KEYS.contains(&k) {
                *later.entry(format!("effect.{k} (R6.4)")).or_default() += 1;
            } else {
                bad.push(format!("{p}: {d}"));
            }
        }
    }
    if flags_missing > 0 {
        later.insert("skill.skillFlags: LuaJIT flags not yet set (R6.4)".into(), flags_missing);
    }

    // --- mainSkill, requirementsTableGems -------------------------------------------------------------------------------
    let main = st.main_skill.map_or(0, |m| m + 1);
    if lj["mainSkill"] != n17(main as f64) {
        bad.push(format!("mainSkill: lj {} rust {main}", lj["mainSkill"]));
    }
    if lj["mainSocketGroup"] != n17(env.main_socket_group) {
        bad.push(format!(
            "mainSocketGroup: lj {} rust {}",
            lj["mainSocketGroup"], env.main_socket_group
        ));
    }
    let mut req = json!(
        env.requirements_table_gems
            .iter()
            .map(|r| {
                let mut o = Map::new();
                o.insert("source".into(), json!(r.source));
                if let Some(g) = r.source_gem {
                    o.insert("sourceGem".into(), gref(g));
                }
                for (k, v) in [("Str", r.str_), ("Dex", r.dex), ("Int", r.int)] {
                    if let Some(v) = v {
                        o.insert(k.into(), n17(v));
                    }
                }
                Json::Object(o)
            })
            .collect::<Vec<_>>()
    );
    mask(&mut req, &amb, &mut bad);
    let mut diffs = Vec::new();
    compare(&lj["requirementsGems"], &req, "requirementsGems", &mut diffs);
    bad.extend(diffs.into_iter().map(|(p, d)| format!("{p}: {d}")));

    // --- env.modDB after initEnv ----------------------------------------------------------------------------------------
    let mut db = db_dump(&env);
    mask(&mut db, &amb, &mut bad);
    let mut want_db = lj["modDB"].clone();
    for (_, b) in want_db["mods"].as_object_mut().unwrap() {
        b.as_array_mut()
            .unwrap()
            .sort_by(|a, b| a.as_str().unwrap().as_bytes().cmp(b.as_str().unwrap().as_bytes()));
    }
    let mut diffs = Vec::new();
    compare(&want_db, &db, "modDB", &mut diffs);
    bad.extend(diffs.into_iter().map(|(p, d)| format!("{p}: {d}")));

    let summary = format!(
        "{name}: groups {} / active skills {} / mainSkill {} / reqGems {} / modDB {} mods (order notes {})",
        groups.len(),
        st.active_skill_list.len(),
        main,
        env.requirements_table_gems.len(),
        db["count"].as_str().unwrap(),
        st.order_notes.len()
    );
    (bad, later, summary)
}

/// LuaJIT's explode source vs Rust's: the cluster node's own `id` must be Rust's node; a spec node (no own `id`) is checked through
/// Rust's node being allocated (`alloc = true` in the dump).
fn explode_identity(lj: &Json, rust: Option<&Json>) -> Json {
    let Some(rust) = rust else { return lj.clone() };
    match (lj.get("id").and_then(Json::as_str), rust.get("$node").and_then(Json::as_u64)) {
        (Some(id), Some(n)) if id == n.to_string() => rust.clone(),
        (None, Some(_)) if lj.get("alloc") == Some(&json!(true)) && lj.get("dn").is_none() => rust.clone(),
        _ => lj.clone(),
    }
}

#[test]
fn skills_equal_luajit_on_six_goldens() {
    let mut all_bad = Vec::new();
    let mut later_all: BTreeMap<String, usize> = BTreeMap::new();
    for g in GOLDEN_BUILDS {
        let (bad, later, summary) = check(g.name);
        println!("{summary}");
        for (k, n) in later {
            *later_all.entry(k).or_default() += n;
        }
        all_bad.extend(bad.into_iter().map(|b| format!("{}: {b}", g.name)));
    }
    println!("waiting for R6.4 / not MAIN (counts over 6 goldens): {later_all:#?}");
    assert!(
        all_bad.is_empty(),
        "{} differences:\n{}",
        all_bad.len(),
        all_bad[..all_bad.len().min(60)].join("\n")
    );
}

/// The comparator is not vacuous: a changed leaf, a missing key and a reordered list are all reported.
#[test]
fn comparator_reports_differences() {
    let want = json!({ "a": "1", "b": ["x", "y"], "c": { "d": true } });
    for got in [
        json!({ "a": "2", "b": ["x", "y"], "c": { "d": true } }),
        json!({ "a": "1", "b": ["y", "x"], "c": { "d": true } }),
        json!({ "a": "1", "b": ["x", "y"], "c": {} }),
        json!({ "a": "1", "b": ["x"], "c": { "d": true } }),
    ] {
        let mut out = Vec::new();
        compare(&want, &got, "t", &mut out);
        assert!(!out.is_empty(), "{got}");
    }
    let mut out = Vec::new();
    compare(&want, &want.clone(), "t", &mut out);
    assert!(out.is_empty());
}

/// Perf (DoD): `init_env` with vs without the skill phase, and `SkillsTab:Load` (+ `UpdateSocketGroups`) alone -- in LuaJIT the load runs
/// at build load, not inside `calcs.initEnv`. Median of 21 runs after a warm-up, ms.
/// `cargo test -p pob-calc --release --test skills_setup -- --ignored --nocapture perf`.
#[test]
#[ignore = "timing run; release build"]
fn perf_skill_phase() {
    use std::time::Instant;
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    for g in GOLDEN_BUILDS {
        let xml = std::fs::read_to_string(g.xml_path()).unwrap();
        let model = Rc::new(pob_build::load_build_xml(&xml).unwrap());
        let items = pob_calc::parse_items(&model, ParseMode::Full);
        let spec = || {
            let mut s = pob_tree::PassiveSpec::load_mode(pob_build::active_spec(&model), ParseMode::Full).unwrap();
            pob_calc::init_env::post_load_spec(&mut s, &items).unwrap();
            s
        };
        let t = |skills: bool| -> f64 {
            let opts = InitEnvOptions {
                spec: Some(spec()),
                items: Some(items.clone()),
                parse_mode: ParseMode::Full,
            };
            let t0 = Instant::now();
            let env = init_env_with(model.clone(), opts, skills).unwrap();
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            drop(env);
            ms
        };
        t(true);
        let (mut on, mut off, mut load) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..21 {
            on.push(t(true));
            off.push(t(false));
        }
        let probe = init_env_with(
            model.clone(),
            InitEnvOptions {
                spec: Some(spec()),
                items: Some(items.clone()),
                parse_mode: ParseMode::Full,
            },
            false,
        )
        .unwrap();
        for _ in 0..21 {
            let t0 = Instant::now();
            let st = SkillsState::load(&model, &probe.ordered_slots, &probe.items).unwrap();
            load.push(t0.elapsed().as_secs_f64() * 1e3);
            drop(st);
        }
        let (on, off, load) = (median(on), median(off), median(load));
        println!(
            "{}: initEnv skills on {on:.3} / off {off:.3} ms -> skill phase {:.3} ms (of which SkillsTab:Load {load:.3}, CalcSetup {:.3})",
            g.name,
            on - off,
            on - off - load
        );
    }
}
