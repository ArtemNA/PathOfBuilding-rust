//! R6.3/R6.4 gate: the skill half of `calcs.initEnv` (CalcSetup.lua :1308-1356, :1448-1945, CalcActiveSkill.lua `createActiveSkill` and
//! `buildActiveSkillModList`) against the LuaJIT fixtures of R6.1 (`tests/fixtures/<build>.skills.json`, producer
//! `tools/lua-corpus/export-skills.ts`), 6 goldens, `ParseMode::Full`, skills on.
//!
//!   * `socketGroups` -- every field the probe dumps (D15/R13: a gem with several candidate variants is masked by the probe; Rust's id must
//!     be one of `ambiguousVariants`), the `srcInstance.skillPart*`/`skillMinion*`/... writes of `buildActiveSkillModList` included;
//!   * every active skill, every field the probe dumps: effects (order; `grantedEffectLevel` = the level table), `skillTypes`, `skillFlags`
//!     (all of them), `skillCfg`/`weapon1Cfg`/`weapon2Cfg` (every field), `weapon1Flags`/`weapon2Flags`, `skillData` (`%.17g`),
//!     `skillModList` (the list's own mods as a multiset of `formatSourceMod`, MINEFIELD-RUST R13; `baseSame`), `buffList` (a multiset of
//!     entries, each `modList` a multiset), `extraSkillModList`, `minionList`, `triggeredBy`, `skillPart*`, `skillTotemId`;
//!   * the totals (`skillModCount`, buffList entries), `mainSkill`, `requirementsTableGems`, `env.modDB` after initEnv (mods as a multiset
//!     per name, conditions, multipliers).
//!
//! What LuaJIT's dump holds that MAIN cannot have is counted per key, not hidden: `displaySkillListCalcs` and the `displayGemList` of a
//! group MAIN does not process (the CALCS-mode initEnv of CalcsTab). A difference on any other key fails.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pob_calc::active_skill::{ActiveSkill, EffectRef, GemCfg, SkillCfg};
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

    /// `grantedEffect.levels[l]` as the probe's `D` dumps it (stat values `[1..n]`, the named fields, `cost` -- `{}` after
    /// ProcessSocketGroup's `levels[l].cost = {}`).
    fn level_table(&self, ge: usize, l: f64) -> Json {
        let g = &self.st.skills.effects[ge];
        let lv = g.level(l).expect("grantedEffectLevel is a level of the effect");
        let mut o = Map::new();
        for (i, v) in lv.values.iter().enumerate() {
            o.insert((i + 1).to_string(), n17(*v));
        }
        for (k, v) in lv.named_numbers() {
            o.insert(k.into(), n17(v));
        }
        if let Some(si) = &lv.stat_interpolation {
            o.insert(
                "statInterpolation".into(),
                Json::Object(si.iter().enumerate().map(|(i, v)| ((i + 1).to_string(), n17(*v))).collect()),
            );
        }
        if self.st.cost_wiped.iter().any(|(id, x)| id == &g.id && *x == l) {
            o.insert("cost".into(), json!({}));
        } else if let Some(c) = &lv.cost {
            o.insert("cost".into(), Json::Object(c.iter().map(|(k, v)| (k.clone(), n17(*v))).collect()));
        }
        Json::Object(o)
    }

    /// `skillCfg` / `weapon<n>Cfg` (`fields(v, { skillTypes = true })`; `skillCond` is the table's own keys -- `pairs` does not follow
    /// `__index`).
    fn cfg(&self, c: &SkillCfg) -> Json {
        let mut o = Map::new();
        o.insert("flags".into(), n17(c.flags));
        o.insert("keywordFlags".into(), n17(c.keyword_flags));
        o.insert("skillName".into(), json!(c.skill_name));
        if let Some(x) = &c.summon_skill_name {
            o.insert("summonSkillName".into(), json!(x));
        }
        if let Some(x) = &c.skill_gem {
            o.insert("skillGem".into(), json!(x));
        }
        o.insert("skillGrantedEffect".into(), json!(self.effect_id(c.skill_granted_effect)));
        if let Some(x) = c.skill_part {
            o.insert("skillPart".into(), n17(x));
        }
        o.insert(
            "skillCond".into(),
            Json::Object(c.skill_cond.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
        );
        if let Some(x) = c.skill_dist {
            o.insert("skillDist".into(), n17(x));
        }
        if let Some(x) = &c.slot_name {
            o.insert("slotName".into(), json!(x));
        }
        if let Some(x) = &c.socket_color {
            o.insert("socketColor".into(), json!(x));
        }
        if let Some(x) = c.socket_num {
            o.insert("socketNum".into(), n17(x));
        }
        Json::Object(o)
    }

    /// `skillDump(sk, i)` of the probe.
    fn skill(&self, i: usize, s: &ActiveSkill) -> Json {
        let mut o = Map::new();
        o.insert("index".into(), n17((i + 1) as f64));
        o.insert("activeEffect".into(), self.effect(&s.active_effect));
        o.insert(
            "effectList".into(),
            json!(s.effect_list.iter().map(|e| self.effect(e)).collect::<Vec<_>>()),
        );
        o.insert(
            "supportList".into(),
            json!(s.support_list.iter().map(|e| self.effect(e)).collect::<Vec<_>>()),
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
        o.insert("actor".into(), json!("<actor>"));
        // the probe's `D` skips every key named `minion` (SKIP), `skillFlags.minion` included
        o.insert(
            "skillFlags".into(),
            Json::Object(
                s.skill_flags
                    .iter()
                    .filter(|(k, _)| k.as_str() != "minion")
                    .map(|(k, v)| (k.clone(), json!(v)))
                    .collect(),
            ),
        );
        if let Some(ml) = &s.skill_mod_list {
            let StoreKind::List(l) = &ml.borrow().kind else {
                panic!("skillModList is a ModList")
            };
            let mut mods: Vec<String> = l.mods.iter().map(|m| format_source_mod(m)).collect();
            mods.sort();
            o.insert("skillModList".into(), json!({ "mods": mods }));
            o.insert("baseSame".into(), json!(true));
        }
        if let Some(c) = &s.skill_cfg {
            o.insert("skillCfg".into(), self.cfg(c));
            o.insert("skillTypesIsCfgTypes".into(), json!(true));
        } else {
            o.insert("skillTypesIsCfgTypes".into(), json!(false));
        }
        if let Some(c) = &s.weapon1_cfg {
            o.insert("weapon1Cfg".into(), self.cfg(c));
        }
        if let Some(c) = &s.weapon2_cfg {
            o.insert("weapon2Cfg".into(), self.cfg(c));
        }
        if let Some(f) = s.weapon1_flags {
            o.insert("weapon1Flags".into(), n17(f64::from(f)));
        }
        if let Some(f) = s.weapon2_flags {
            o.insert("weapon2Flags".into(), n17(f64::from(f)));
        }
        o.insert(
            "skillData".into(),
            Json::Object(s.skill_data.iter().map(|(k, v)| (k.clone(), lua_value(v))).collect()),
        );
        let mut buffs: Vec<Json> = s
            .buff_list
            .iter()
            .map(|b| {
                let mut e = Map::new();
                for (k, v) in [
                    ("type", &b.ty),
                    ("name", &b.name),
                    ("allowTotemBuff", &b.allow_totem_buff),
                    ("cond", &b.cond),
                    ("enemyCond", &b.enemy_cond),
                    ("stackVar", &b.stack_var),
                    ("stackLimit", &b.stack_limit),
                    ("stackLimitVar", &b.stack_limit_var),
                    ("applyNotPlayer", &b.apply_not_player),
                    ("applyMinions", &b.apply_minions),
                    ("applyAllies", &b.apply_allies),
                ] {
                    if !v.is_nil() {
                        e.insert(k.into(), lua_value(v));
                    }
                }
                if let Some(a) = b.active_skill_buff {
                    e.insert("activeSkillBuff".into(), json!(a));
                }
                let mut mods: Vec<String> = b.mod_list.iter().map(|m| format_source_mod(m)).collect();
                mods.sort();
                e.insert("modList".into(), json!(mods));
                Json::Object(e)
            })
            .collect();
        buffs.sort_by_key(canon);
        o.insert("buffList".into(), json!(buffs));
        if let Some(l) = &s.extra_skill_mod_list {
            let mut mods: Vec<String> = l.iter().map(|m| format_source_mod(m)).collect();
            mods.sort();
            o.insert("extraSkillModList".into(), json!(mods));
        }
        if let Some(l) = &s.minion_list {
            o.insert(
                "minionList".into(),
                Json::Object(l.iter().enumerate().map(|(i, m)| ((i + 1).to_string(), json!(m))).collect()),
            );
        }
        if s.minion.is_some() {
            o.insert("minion".into(), json!("<minion>"));
        }
        if let Some(e) = &s.triggered_by {
            o.insert("triggeredBy".into(), self.brief(e));
        }
        if let Some(x) = s.skill_part {
            o.insert("skillPart".into(), n17(x));
        }
        if let Some(x) = &s.skill_part_name {
            o.insert("skillPartName".into(), json!(x));
        }
        if let Some(x) = s.skill_totem_id {
            o.insert("skillTotemId".into(), n17(x));
        }
        if let Some(x) = &s.disable_reason {
            o.insert("disableReason".into(), json!(x));
        }
        if let Some(x) = s.active_mine_count {
            o.insert("activeMineCount".into(), n17(x));
        }
        if let Some(x) = s.active_stage_count {
            o.insert("activeStageCount".into(), n17(x));
        }
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
        num("actorLevel", e.actor_level);
        if let Some(l) = e.granted_effect_level {
            o.insert("grantedEffectLevel".into(), self.level_table(e.granted_effect, l));
        }
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

/// A canonical text of a JSON value (object keys sorted) -- the sort key of a multiset of tables.
fn canon(j: &Json) -> String {
    match j {
        Json::Object(o) => {
            let mut ks: Vec<&String> = o.keys().collect();
            ks.sort();
            let parts: Vec<String> = ks.iter().map(|k| format!("{}:{}", json!(k), canon(&o[k.as_str()]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Json::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

/// Proven class `stale-perform-tag`: CalcPerform.lua:2382 writes `warcryBuff[1].warcryPowerBonus` onto the GlobalEffect tag TABLE of a
/// warcry buff mod -- a table shared with the data's statMap mod (`mergeLevelMod` copies the mod with `copyTable(mod, true)`, the tags stay
/// the same objects). The write happens in every perform pass (branch `not CannotShareWarcryBuffs`) before both reads of that pass (:2398,
/// :2411); the reads :3090/:3099 are the party warcries (partyTab, not ported, empty). So the value left by the previous perform of the
/// LuaJIT process (the load ran MAIN and CALCS before the probe's initEnv) never changes a result -- it is only visible in a dump of the
/// state after initEnv. Rust's run is the first (fresh data: no such field). Modelling the write (a shared tag cell, like `div`, R4) is
/// R6.7. The pin: LuaJIT's mod with that one field removed must equal Rust's mod (the comparison that follows); every place is listed by
/// name in [`STALE_PINS`] -- any other difference or another count fails.
const STALE: &str = "stale-perform-tag";
const STALE_PINS: [&str; 1] = ["OccVortex, activeSkills[4] Enduring Cry, buffList[1].modList[0]"];

/// Removes `/warcryPowerBonus=<n>` from the GlobalEffect tags with `effectType=Warcry` of LuaJIT's buff mods; returns where it did.
fn strip_stale_perform_tags(w: &mut Json) -> Vec<String> {
    let mut pins = Vec::new();
    let Some(buffs) = w.get_mut("buffList").and_then(Json::as_array_mut) else {
        return pins;
    };
    for (b, buff) in buffs.iter_mut().enumerate() {
        let Some(mods) = buff.get_mut("modList").and_then(Json::as_array_mut) else {
            continue;
        };
        for (m, md) in mods.iter_mut().enumerate() {
            let Some(text) = md.as_str() else { continue };
            let Some((head, tags)) = text.rsplit_once('|') else { continue };
            let new_tags: Vec<String> = tags
                .split(',')
                .map(|t| {
                    let parts: Vec<&str> = t.split('/').collect();
                    if parts.contains(&"type=GlobalEffect") && parts.contains(&"effectType=Warcry") {
                        parts
                            .iter()
                            .filter(|p| !p.starts_with("warcryPowerBonus="))
                            .copied()
                            .collect::<Vec<_>>()
                            .join("/")
                    } else {
                        t.to_owned()
                    }
                })
                .collect();
            let new_text = format!("{head}|{}", new_tags.join(","));
            if new_text != text {
                pins.push(format!("buffList[{b}].modList[{m}]"));
                *md = json!(new_text);
            }
        }
        mods.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    }
    pins
}

/// LuaJIT's skill dump made order-free where LuaJIT's order is per process (R13): `buffList` entries sorted canonically (the probe sorts
/// by its own serialisation; the mods inside are already sorted).
fn canon_skill(w: &mut Json) {
    if let Some(b) = w.get_mut("buffList").and_then(Json::as_array_mut) {
        b.sort_by_key(canon);
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
    let processed_in_main = |gi: usize| -> bool {
        let sg = &groups[gi];
        (gi + 1) as f64 == env.main_socket_group || (sg.enabled && sg.slot_enabled == Some(true))
    };
    compare(&want_groups, &got_groups, "socketGroups", &mut diffs);
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
    let mut n_mods = 0usize;
    let mut n_buffs = 0usize;
    for (i, (w, s)) in want_skills.iter().zip(&st.active_skill_list).enumerate() {
        let got = cx.skill(i, s);
        n_mods += got["skillModList"]["mods"].as_array().map_or(0, Vec::len);
        n_buffs += got["buffList"].as_array().map_or(0, Vec::len);
        let mut got = got;
        mask(&mut got, &amb, &mut bad);
        let mut want = w.clone();
        canon_skill(&mut want);
        for pin in strip_stale_perform_tags(&mut want) {
            *later
                .entry(format!(
                    "{STALE}: {name}, activeSkills[{i}] {}, {pin}",
                    s.skill_cfg.as_ref().map_or("", |c| c.skill_name.as_str())
                ))
                .or_default() += 1;
        }
        let mut diffs = Vec::new();
        compare(&want, &got, &format!("activeSkills[{i}]"), &mut diffs);
        bad.extend(diffs.into_iter().map(|(p, d)| format!("{p}: {d}")));
    }
    let lj_mods: usize = want_skills
        .iter()
        .map(|w| w["skillModList"]["mods"].as_array().map_or(0, Vec::len))
        .sum();
    let lj_buffs: usize = want_skills.iter().map(|w| w["buffList"].as_array().map_or(0, Vec::len)).sum();
    if lj["skillModCount"] != n17(n_mods as f64) || lj_mods != n_mods || lj_buffs != n_buffs {
        bad.push(format!(
            "totals: skillModList mods lj {} ({lj_mods}) rust {n_mods}, buffList entries lj {lj_buffs} rust {n_buffs}",
            lj["skillModCount"]
        ));
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
        "{name}: groups {} / active skills {} / skillModList mods {n_mods} / buffList entries {n_buffs} / auxSkillList {} / mainSkill {} / reqGems {} / modDB {} mods (order notes {})",
        groups.len(),
        st.active_skill_list.len(),
        st.aux_skill_list.len(),
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
    println!("not MAIN / proven classes (counts over 6 goldens): {later_all:#?}");
    let stale: Vec<String> = later_all
        .iter()
        .filter(|(k, _)| k.starts_with(STALE))
        .map(|(k, n)| {
            assert_eq!(*n, 1, "{k}");
            k[STALE.len() + 2..].to_owned()
        })
        .collect();
    assert_eq!(stale, STALE_PINS.map(str::to_owned).to_vec(), "{STALE}: the pinned places, by name");
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

/// `processMod` in a lazy statMap copy sets `grantedEffect.hasGlobalEffect` (Data.lua:1033-1045), and CalcSetup.lua:1743 reads it to skip a
/// global-effect skill whose `enableGlobal` is off. LuaJIT's fixture is at least the third initEnv of its process (the load ran MAIN and
/// CALCS), Rust's is the first: a mark that `buildActiveSkillModList` sets on an effect some gem of the build grants would make the two
/// differ. Measured per golden on a fresh thread (fresh `data.skills`): the effects whose mark the run flipped, and how many of them a gem
/// of the build grants with its `enableGlobal<k>` off (the only case :1755 reads the mark) -- must be 0.
#[test]
fn lazy_global_effect_marks_do_not_reach_the_builds() {
    for g in GOLDEN_BUILDS {
        let name = g.name;
        let (flipped, used) = std::thread::spawn(move || {
            let sk = pob_data::skills::skills();
            let before: Vec<bool> = sk.effects.iter().map(|e| e.has_global_effect.get()).collect();
            let env = run(name);
            let st = env.skills.as_ref().expect("skills on");
            let flipped: Vec<usize> = sk
                .effects
                .iter()
                .enumerate()
                .filter(|(i, e)| e.has_global_effect.get() != before[*i])
                .map(|(i, _)| i)
                .collect();
            let mut used = Vec::new();
            for sg in st.tab.socket_group_list() {
                for gi in &sg.gem_list {
                    let ids: Vec<String> = match &gi.gem_data {
                        Some(gd) => {
                            let e = &gems().gems[gd];
                            std::iter::once(e.granted_effect_id.clone())
                                .chain(e.secondary_granted_effect_id.clone())
                                .collect()
                        }
                        None => gi.granted_effect.iter().cloned().collect(),
                    };
                    for (k, id) in ids.into_iter().enumerate() {
                        // CalcSetup.lua:1755 `not grantedEffect.hasGlobalEffect or gemInstance["enableGlobal"..index]`
                        let enable_global = if k == 0 { gi.enable_global1 } else { gi.enable_global2 } == Some(true);
                        if let Some(&i) = sk.index.get(&id)
                            && flipped.contains(&i)
                            && !enable_global
                        {
                            used.push(id);
                        }
                    }
                }
            }
            (flipped.iter().map(|&i| sk.effects[i].id.clone()).collect::<Vec<_>>(), used)
        })
        .join()
        .expect("thread");
        println!(
            "{name}: hasGlobalEffect flipped by the run on {} effects {flipped:?}; granted with enableGlobal off: {used:?}",
            flipped.len()
        );
        assert!(used.is_empty(), "{name}: {used:?}");
    }
}
