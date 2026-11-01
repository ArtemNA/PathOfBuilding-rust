//! The JSON shapes of the `engine_*JSON` readers (boot.lua), built from a Rust run. Key order: the Lua tables are hash tables
//! (dkjson walks `pairs`), so the order of object keys carries no meaning; arrays keep the Lua order.

use pob_calc::pob_build::{ConfigState, InputValue};
use pob_calc::{CalculateResult, FullDpsSkill, StatValue};
use serde_json::{Map, Value as Json, json};

use crate::display::DisplaySection;

fn num(n: f64) -> Json {
    serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number)
}

/// A Lua number as boot.lua serialises it (JSON has no NaN/inf: `tostring(v)`).
fn lua_num(n: f64) -> Json {
    if n.is_finite() {
        num(n)
    } else {
        Json::String(pob_lua::tostring_num(n))
    }
}

pub fn input_json(v: &InputValue) -> Json {
    match v {
        InputValue::Num(n) => lua_num(*n),
        InputValue::Str(s) => Json::String(s.clone()),
        InputValue::Bool(b) => Json::Bool(*b),
    }
}

/// The scalars of an output as `engine_outputJSON` writes them.
pub fn stats_json(stats: &indexmap::IndexMap<String, StatValue>) -> Json {
    let mut flat = Map::new();
    for (k, v) in stats {
        let j = match v {
            StatValue::Num(n) => lua_num(*n),
            StatValue::Str(s) => Json::String(s.clone()),
            StatValue::Bool(b) => Json::Bool(*b),
        };
        flat.insert(k.clone(), j);
    }
    Json::Object(flat)
}

/// `engine_outputJSON` (boot.lua:790-822): the scalars of `mainOutput` + `_meta`.
pub fn output_json(res: &CalculateResult) -> Json {
    let Json::Object(mut flat) = stats_json(&res.stats) else {
        unreachable!("an object")
    };
    let m = &res.meta;
    flat.insert(
        "_meta".into(),
        json!({
            "allocNodes": m.alloc_nodes,
            "items": m.items,
            "socketGroups": m.socket_groups,
            "mainSocketGroup": num(m.main_socket_group),
            "treeVersion": m.tree_version,
            "className": m.class_name,
            "ascendName": m.ascend_name,
        }),
    );
    Json::Object(flat)
}

/// `engine_fullDPSJSON` (boot.lua:506-520).
pub fn full_dps_json(list: &[FullDpsSkill]) -> Json {
    Json::Array(
        list.iter()
            .map(|s| {
                let mut o = Map::new();
                o.insert("name".into(), Json::String(s.name.clone()));
                o.insert("dps".into(), lua_num(s.dps));
                o.insert("count".into(), lua_num(s.count));
                if let Some(t) = &s.trigger {
                    o.insert("trigger".into(), Json::String(t.clone()));
                }
                if let Some(p) = &s.skill_part {
                    o.insert("skillPart".into(), Json::String(p.clone()));
                }
                if let Some(src) = &s.source {
                    o.insert("source".into(), Json::String(src.clone()));
                }
                Json::Object(o)
            })
            .collect(),
    )
}

/// `engine_displayStatsJSON` (boot.lua:776-788).
pub fn display_json(sections: &[DisplaySection]) -> Json {
    Json::Array(
        sections
            .iter()
            .map(|s| {
                json!({
                    "actor": s.actor,
                    "title": s.title,
                    "rows": s.rows.iter().map(|r| {
                        let mut o = Map::new();
                        o.insert("label".into(), Json::String(r.label.clone()));
                        o.insert("value".into(), Json::String(r.value.clone()));
                        o.insert("color".into(), Json::String(r.color.clone()));
                        o.insert("valueColor".into(), Json::String(r.value_color.clone()));
                        if let Some(k) = r.stat_key {
                            o.insert("statKey".into(), Json::String(k.to_owned()));
                        }
                        Json::Object(o)
                    }).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// `engine_skillListJSON` (boot.lua:369-403) over `build.skillsTab.socketGroupList` after the MAIN run.
pub fn skill_list_json(res: &CalculateResult) -> Json {
    let Some(skills) = res.env.skills.as_ref() else {
        return Json::Array(Vec::new());
    };
    let data = pob_data::skills::skills();
    let gems = pob_data::gems::gems();
    let main = res.env.main_socket_group;
    let groups = skills.tab.socket_group_list();
    Json::Array(
        groups
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let index = i + 1;
                let gem_list: Vec<Json> = g
                    .gem_list
                    .iter()
                    .map(|gi| {
                        // `(gemData and gemData.grantedEffect) or gemInstance.grantedEffect`
                        let effect = gi
                            .gem_data
                            .as_ref()
                            .and_then(|id| gems.gems.0.get(id))
                            .map(|d| d.granted_effect_id.clone())
                            .or_else(|| gi.granted_effect.clone());
                        let support = effect
                            .and_then(|e| data.index.get(&e).map(|&x| data.effects[x].support))
                            .unwrap_or(false);
                        let mut o = Map::new();
                        if let Some(n) = &gi.name_spec {
                            o.insert("name".into(), Json::String(n.clone()));
                        }
                        if let Some(l) = gi.level {
                            o.insert("level".into(), lua_num(l));
                        }
                        if let Some(q) = gi.quality {
                            o.insert("quality".into(), lua_num(q));
                        }
                        o.insert("enabled".into(), Json::Bool(gi.enabled));
                        o.insert("isSupport".into(), Json::Bool(support));
                        Json::Object(o)
                    })
                    .collect();
                let label = g
                    .display_label
                    .clone()
                    .or_else(|| g.label.clone())
                    .unwrap_or_else(|| format!("Group {index}"));
                json!({
                    "index": index,
                    "label": label,
                    "enabled": g.enabled,
                    "includeInFullDPS": g.include_in_full_dps == Some(true),
                    "isMainGroup": index as f64 == main,
                    "gems": gem_list,
                })
            })
            .collect(),
    )
}

/// `configVarByName[id]` (boot.lua:233-236).
pub fn config_var_known(id: &str) -> bool {
    pob_data::config_options::config_options()
        .vars
        .iter()
        .any(|v| v.var.as_deref() == Some(id))
}

/// `engine_configListJSON` (boot.lua:249-322), without `eligible`: the ConfigTab control's `shown` closure (ConfigTab.lua's `if*`
/// gates over the last run) is not ported -- the key is left out rather than guessed.
pub fn config_list_json(c: &ConfigState) -> Json {
    let mut out = Vec::new();
    let defaults = crate::save::default_state();
    let strip = |s: &str| crate::display::strip_escapes(s);
    for v in &pob_data::config_options::config_options().vars {
        let (Some(var), Some(ty)) = (&v.var, &v.ty) else { continue };
        let raw = c.input.get(var);
        let value = raw.or_else(|| c.placeholder.get(var)).or_else(|| defaults.get(var));
        let is_set = raw.is_some_and(|r| Some(r) != defaults.get(var));
        let mut o = Map::new();
        o.insert("id".into(), Json::String(var.clone()));
        o.insert("type".into(), Json::String(ty.clone()));
        o.insert("label".into(), Json::String(v.label.as_deref().map_or_else(|| var.clone(), strip)));
        o.insert("section".into(), Json::String(v.section.clone()));
        if let Some(x) = value {
            o.insert("value".into(), input_json(x));
        }
        o.insert("isSet".into(), Json::Bool(is_set));
        if ty == "list"
            && let Some(l) = &v.list
        {
            let opts: Vec<Json> = l
                .iter()
                .map(|e| {
                    let val = match &e.val {
                        pob_data::wire::Scalar::Num(n) => lua_num(*n),
                        pob_data::wire::Scalar::Str(s) => Json::String(s.clone()),
                        pob_data::wire::Scalar::Bool(b) => Json::Bool(*b),
                    };
                    json!({ "value": val, "label": strip(&e.label) })
                })
                .collect();
            o.insert("options".into(), Json::Array(opts));
        }
        out.push(Json::Object(o));
    }
    Json::Array(out)
}
