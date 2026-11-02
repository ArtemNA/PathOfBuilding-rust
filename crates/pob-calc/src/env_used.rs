//! Calcs.lua:467-609, the `env.skillsUsed` / `env.conditionsUsed` / ... tables of the MAIN tail: what the ConfigTab's `if*` gates read as `mainEnv.xUsed[name]`. A reader over
//! the finished env. Lua keeps the mod lists for the dev-mode tooltip (`launch.devModeAlt`, never on in the port); the gates only test presence, so the walk order (`pairs`,
//! R13) does not matter.

use std::collections::{HashMap, HashSet};

use pob_mods::{Mod, StoreKind, StoreRef, Tag, Value};

use crate::env::Env;

const CALCS: &str = "Modules/Calcs.lua";

/// The names of each table.
#[derive(Debug, Default)]
pub struct EnvUsed {
    pub skills: HashSet<String>,
    pub conditions: HashSet<String>,
    pub enemy_conditions: HashSet<String>,
    pub minion_conditions: HashSet<String>,
    pub multipliers: HashSet<String>,
    pub enemy_multipliers: HashSet<String>,
    pub per_stats: HashSet<String>,
    pub enemy_per_stats: HashSet<String>,
    pub tag_types: HashSet<String>,
    pub mods: HashSet<String>,
    /// `mod.name:lower():match(...)` of :521-527, one answer per name.
    ailments: HashMap<String, (bool, bool)>,
}

fn key(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        // `out[nil] = { }`
        Value::Nil => panic!("{CALCS}:494: table index is nil"),
        other => unimplemented!("{CALCS}:494 a used table keyed by a {}", other.type_name()),
    }
}

/// :488-497 `addTo`: a Base mod only describes.
fn add_to(out: &mut HashSet<String>, var: &Value, m: &Mod) {
    if m.source.as_deref() == Some("Base") {
        return;
    }
    out.insert(key(var));
}

/// :498-506 `addVarTag`.
fn add_var_tag(out: &mut HashSet<String>, tag: &Tag, m: &Mod) {
    match tag.get("varList") {
        Value::Table(t) => {
            for v in &t.array {
                add_to(out, v, m);
            }
        }
        _ => add_to(out, &tag.get("var"), m),
    }
}

/// :507-515 `addStatTag`.
fn add_stat_tag(out: &mut HashSet<String>, tag: &Tag, m: &Mod) {
    if tag.get("varList").truthy() {
        if let Value::Table(t) = tag.get("statList") {
            for v in &t.array {
                add_to(out, v, m);
            }
        }
    } else if tag.get("stat").truthy() {
        add_to(out, &tag.get("stat"), m);
    }
}

/// :521-527: the elemental ailments a mod's name implies.
fn name_ailments(name: &str) -> (bool, bool) {
    let lower = name.to_ascii_lowercase();
    let is = |p: &[u8]| pob_lua::match_(lower.as_bytes(), p, None).expect("Calcs.lua:522 pattern").is_some();
    (is(b"[fi][ig][rn][ei]t?e?"), is(b"[cf][or][le][de]z?e?"))
}

/// :516-563 `addModTags`.
fn add_mod_tags(out: &mut EnvUsed, is_player: bool, m: &Mod) {
    add_to(&mut out.mods, &Value::Str(m.name.clone()), m);
    let (ignite, freeze) = match out.ailments.get(&*m.name) {
        Some(a) => *a,
        None => {
            let a = name_ailments(&m.name);
            out.ailments.insert(m.name.to_string(), a);
            a
        }
    };
    if ignite {
        add_to(&mut out.enemy_conditions, &Value::str("Ignited"), m);
        add_to(&mut out.enemy_conditions, &Value::str("Burning"), m);
    }
    if freeze {
        add_to(&mut out.enemy_conditions, &Value::str("Frozen"), m);
    }
    for tag in &m.tags {
        let ty = tag.ty();
        add_to(&mut out.tag_types, ty, m);
        let actor = tag.get("actor");
        match ty.as_str() {
            Some("IgnoreCond") => break,
            Some("Condition") => {
                if is_player {
                    add_var_tag(&mut out.conditions, tag, m);
                } else {
                    add_var_tag(&mut out.minion_conditions, tag, m);
                }
            }
            Some("ActorCondition") if tag.get("var").truthy() => {
                if actor.as_str() == Some("enemy") {
                    add_to(&mut out.enemy_conditions, &tag.get("var"), m);
                } else {
                    add_to(&mut out.conditions, &tag.get("var"), m);
                }
            }
            Some("Multiplier" | "MultiplierThreshold") => {
                if !actor.truthy() {
                    if is_player {
                        add_var_tag(&mut out.multipliers, tag, m);
                    }
                } else if actor.as_str() == Some("enemy") {
                    add_var_tag(&mut out.enemy_multipliers, tag, m);
                }
            }
            Some("PerStat" | "StatThreshold") => {
                if !actor.truthy() {
                    if is_player {
                        add_stat_tag(&mut out.per_stats, tag, m);
                    }
                } else if actor.as_str() == Some("enemy") {
                    add_stat_tag(&mut out.enemy_per_stats, tag, m);
                }
            }
            _ => {}
        }
    }
}

fn for_each_mod(store: &StoreRef, mut f: impl FnMut(&Mod)) {
    let s = store.borrow();
    match &s.kind {
        StoreKind::Db(d) => {
            for list in d.mods.values() {
                for m in list {
                    f(m);
                }
            }
        }
        StoreKind::List(l) => {
            for m in &l.mods {
                f(m);
            }
        }
    }
}

/// Calcs.lua:467-609 over the finished MAIN env.
pub fn env_used(env: &Env) -> EnvUsed {
    let mut out = EnvUsed::default();
    if let Some(st) = &env.skills {
        // :467-477
        for sk in &st.active_skill_list {
            for e in &sk.effect_list {
                out.skills.insert(st.skills.effects[e.0.borrow().granted_effect].name.to_string());
            }
            if let Some(mn) = &sk.minion {
                for ms in &mn.active_skill_list {
                    out.skills
                        .insert(st.skills.effects[ms.active_effect.0.borrow().granted_effect].id.to_string());
                }
            }
        }
    }
    // :564-570 `ipairs({ env.player, env.minion })`
    let db = env.player.borrow().mod_db.clone();
    for_each_mod(&db, |m| add_mod_tags(&mut out, true, m));
    if let Some(minion) = &env.minion {
        let db = minion.borrow().mod_db.clone();
        for_each_mod(&db, |m| add_mod_tags(&mut out, false, m));
    }
    // :571-588
    if let Some(st) = &env.skills {
        for (i, sk) in st.active_skill_list.iter().enumerate() {
            if let Some(l) = &sk.skill_mod_list {
                for_each_mod(l, |m| add_mod_tags(&mut out, true, m));
            }
            if let Some(l) = env.skill_mod_lists.get(i) {
                for_each_mod(l, |m| {
                    add_to(&mut out.mods, &Value::Str(m.name.clone()), m);
                    for tag in &m.tags {
                        add_to(&mut out.tag_types, tag.ty(), m);
                    }
                });
            }
            if let Some(mn) = &sk.minion {
                for ms in &mn.active_skill_list {
                    if let Some(l) = &ms.skill_mod_list {
                        for_each_mod(l, |m| add_mod_tags(&mut out, false, m));
                    }
                }
            }
        }
    }
    // :589-609
    for_each_mod(&env.enemy_db, |m| {
        for tag in &m.tags {
            let actor = tag.get("actor");
            match tag.ty().as_str() {
                Some("IgnoreCond") => break,
                Some("Condition") => add_var_tag(&mut out.enemy_conditions, tag, m),
                Some("ActorCondition") if tag.get("var").truthy() => {
                    if matches!(actor.as_str(), Some("enemy" | "player")) {
                        add_to(&mut out.conditions, &tag.get("var"), m);
                    } else {
                        add_to(&mut out.enemy_conditions, &tag.get("var"), m);
                    }
                }
                Some("Multiplier" | "MultiplierThreshold") if !actor.truthy() => add_var_tag(&mut out.enemy_multipliers, tag, m),
                _ => {}
            }
        }
    });
    out
}
