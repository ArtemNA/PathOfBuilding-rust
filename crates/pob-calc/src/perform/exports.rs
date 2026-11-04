//! `buffExports` of `calcs.perform` (CalcPerform.lua:2233, :3838-3958) and `PartyTab:setBuffExports` / `exportBuffs` (PartyTab.lua:988-1047): what a support character
//! writes into `<Party><ExportedBuffs>` (ImportTab `Export Support`, `partyTabEnableExportBuffs`, :1312).
//!
//! * Which run: `setBuffExports` keeps the last perform whose mode is not CALCULATOR. `calcsTab:BuildOutput` ends with `buildOutput("CALCS")`: its nested runs (mode CALCS) finish before the
//!   tail of the outer perform, and its `calcFullDPS` is CALCULATOR, so the last writer is always the outer perform of the CALCS run: [`Env::buff_exports`] of that run.
//! * `exportBuffs` formats the mods when `SaveDB` asks, and replaces the table with the text (`ConvertedToText`); the mods here are the same `Rc`s, formatted when asked. What a later
//!   CALCULATOR run writes into a shared tag (`warcryPowerBonus`, `div`) reaches the text in both, except that upstream's second `SaveDB` keeps the first text: not modelled, no override
//!   of a warcry's power reaches a kept tag on the save paths.
//! * Order: the text is `pairsSortByKey` (bytes), and `ipairs` inside; the `pairs` of the tail (`enemyDB.mods`, `buffExports.Aura`, `buffExports.Link`) only fill keys, none depends on the walk.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_lua::tostring_num;
use pob_mods::format::{format_source_mod, format_value};
use pob_mods::{Mod, StoreKind, Str, Tag, Value};

use super::{PerformState, flag};
use crate::env::Env;
use crate::output::OutValue;

const CP: &str = "Modules/CalcPerform.lua";

/// One entry `{ effectMult, modList, isMark }` of `buffExports[type]`.
#[derive(Clone, Debug, Default)]
pub struct ExportedBuff {
    pub effect_mult: f64,
    pub mods: Vec<Rc<Mod>>,
    pub is_mark: bool,
}

/// `buffExports["EnemyMods"][k]`: the mod, or `{ MultiStat = true, mod, mod, ... }` (:3855-3862).
#[derive(Clone, Debug)]
pub enum EnemyExport {
    One(Rc<Mod>),
    Multi(Vec<Rc<Mod>>),
}

/// The `buffType` of `exportBuffs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuffType {
    PlayerMods,
    Aura,
    Curse,
    Warcry,
    Link,
    EnemyConditions,
    EnemyMods,
}

/// `buffExports` (:2233). A sorted map is `pairsSortByKey`; `otherEffects` is a key of `Aura` upstream, kept apart for its other shape.
#[derive(Clone, Debug, Default)]
pub struct BuffExports {
    pub aura: BTreeMap<String, ExportedBuff>,
    pub other_effects: Option<BTreeMap<String, ExportedBuff>>,
    pub curse: BTreeMap<String, ExportedBuff>,
    pub warcry: BTreeMap<String, ExportedBuff>,
    pub link: BTreeMap<String, ExportedBuff>,
    pub enemy_mods: BTreeMap<String, EnemyExport>,
    pub enemy_conditions: BTreeSet<String>,
    pub player_mods: BTreeSet<String>,
    /// `buffExports["CurseLimit"]` (:3244).
    pub curse_limit: f64,
}

impl BuffExports {
    /// :2525-2528 / :2922-2925: an aura of this name made earlier moves to `<name>_Debuff`.
    pub(super) fn set_aura(&mut self, name: &str, effect_mult: f64, mods: Vec<Rc<Mod>>) {
        if let Some(old) = self.aura.remove(name) {
            self.aura.insert(format!("{name}_Debuff"), old);
        }
        self.aura.insert(
            name.to_owned(),
            ExportedBuff {
                effect_mult,
                mods,
                is_mark: false,
            },
        );
    }

    /// `buffExports["Aura"][buff.name..(buffExports["Aura"][buff.name] and "_Debuff" or "")] = { ... }` (:2538, :2604, :2661, :2994).
    pub(super) fn set_aura_debuff(&mut self, name: &str, effect_mult: f64, mods: Vec<Rc<Mod>>) {
        let key = if self.aura.contains_key(name) {
            format!("{name}_Debuff")
        } else {
            name.to_owned()
        };
        self.aura.insert(
            key,
            ExportedBuff {
                effect_mult,
                mods,
                is_mark: false,
            },
        );
    }

    /// :2372-2373 / :2841-2842.
    pub(super) fn set_other_effect(&mut self, name: &str, effect_mult: f64, mods: Vec<Rc<Mod>>) {
        self.other_effects.get_or_insert_default().insert(
            name.to_owned(),
            ExportedBuff {
                effect_mult,
                mods,
                is_mark: false,
            },
        );
    }

    /// `PartyTab:exportBuffs(buffType)` (PartyTab.lua:996-1047), with `enableExportBuffs` and `buffExports[buffType]` already true.
    pub fn export(&self, ty: BuffType) -> String {
        let mut buf = String::new();
        let line = |buf: &mut String, s: &str| {
            buf.push('\n');
            buf.push_str(s);
        };
        // :1006-1009
        let name_line = |buf: &mut String, name: &str| {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(name);
        };
        let mods_lines = |buf: &mut String, mods: &[Rc<Mod>]| {
            for m in mods {
                buf.push('\n');
                buf.push_str(&format_source_mod(m));
            }
            buf.push_str("\n---");
        };
        match ty {
            BuffType::PlayerMods => {
                for k in &self.player_mods {
                    name_line(&mut buf, k);
                }
            }
            BuffType::EnemyConditions => {
                for k in &self.enemy_conditions {
                    name_line(&mut buf, k);
                }
            }
            BuffType::EnemyMods => {
                for (k, e) in &self.enemy_mods {
                    name_line(&mut buf, k);
                    match e {
                        EnemyExport::One(m) => line(&mut buf, &format_source_mod(m)),
                        EnemyExport::Multi(l) => {
                            for m in l {
                                line(&mut buf, &format_source_mod(m));
                            }
                        }
                    }
                }
            }
            BuffType::Curse => {
                // :1003
                buf.push_str("--- Curse Limit ---\n");
                buf.push_str(&tostring_num(self.curse_limit));
                for (k, b) in &self.curse {
                    name_line(&mut buf, k);
                    line(&mut buf, &tostring_num(b.effect_mult * 100.0));
                    line(&mut buf, if b.is_mark { "true" } else { "false" });
                    mods_lines(&mut buf, &b.mods);
                }
            }
            BuffType::Warcry | BuffType::Link => {
                let map = if ty == BuffType::Warcry { &self.warcry } else { &self.link };
                for (k, b) in map {
                    name_line(&mut buf, k);
                    line(&mut buf, &tostring_num(b.effect_mult * 100.0));
                    mods_lines(&mut buf, &b.mods);
                }
            }
            BuffType::Aura => {
                let mut names: Vec<&str> = self.aura.keys().map(String::as_str).collect();
                if self.other_effects.is_some() {
                    names.push("otherEffects");
                }
                names.sort_unstable();
                for k in names {
                    if k == "otherEffects" {
                        // :1020-1026
                        name_line(&mut buf, k);
                        for (inner, b) in self.other_effects.iter().flatten() {
                            for m in &b.mods {
                                line(
                                    &mut buf,
                                    &format!("{inner}|{}|{}", tostring_num(b.effect_mult * 100.0), format_source_mod(m)),
                                );
                            }
                        }
                        buf.push_str("\n---");
                        continue;
                    }
                    let b = &self.aura[k];
                    // :1005 `buffName ~= "extraAura" or #buff.modList > 0`
                    if k == "extraAura" {
                        if b.mods.is_empty() {
                            continue;
                        }
                        name_line(&mut buf, k);
                    } else {
                        name_line(&mut buf, k);
                        line(&mut buf, &tostring_num(b.effect_mult * 100.0));
                    }
                    mods_lines(&mut buf, &b.mods);
                }
            }
        }
        buf
    }
}

/// `tostring(output[key])`.
fn tostring_out(v: Option<&OutValue>) -> String {
    match v {
        None => "nil".to_owned(),
        Some(OutValue::Num(n)) => tostring_num(*n),
        Some(OutValue::Bool(b)) => b.to_string(),
        Some(OutValue::Str(s)) => s.to_string(),
    }
}

/// `local output = env.player.output` (:1308) and its `MainHand`, when `calcs.mirages` has given `env.player.output` another table (CalcMirages.lua:157/:235): the tail reads the table it
/// started with, as it was then. `None` = the same table.
pub(super) struct OwnOutput {
    pub scalars: IndexMap<Str, OutValue>,
    pub main_hand: Option<IndexMap<Str, OutValue>>,
}

/// :3893-3931 a stack-scaled copy of one mine aura (`copyTable`; `tag.effectStackVar = nil`, `mod.value = mod.value * stackCount`).
fn pre_stack(entry: &mut ExportedBuff, stack: &Value) {
    let mods: Vec<Rc<Mod>> = entry
        .mods
        .iter()
        .map(|m| {
            let mut c = m.deep_copy();
            if let Some(i) = c
                .tags
                .iter()
                .position(|t| t.get("effectStackVar").as_str() == Some("ActiveMineCount"))
            {
                let mut t = (*c.tags[i].table()).clone();
                t.hash.shift_remove("effectStackVar");
                c.tags[i] = Tag::new(t);
                let (Some(v), Some(s)) = (c.value.as_num(), stack.as_num()) else {
                    panic!(
                        "{CP}:3907: attempt to perform arithmetic on a {} value",
                        if c.value.as_num().is_none() {
                            c.value.type_name()
                        } else {
                            stack.type_name()
                        }
                    )
                };
                c.value = Value::Num(v * s);
            }
            Rc::new(c)
        })
        .collect();
    entry.mods = mods;
}

/// :3838-3957 the tail of a perform with `partyTabEnableExportBuffs`.
pub(super) fn finish(env: &Env, state: &PerformState, x: &mut BuffExports, own: Option<&OwnOutput>) {
    let output = |k: &str| -> Option<&OutValue> {
        match own {
            Some(o) => o.scalars.get(k),
            None => state.player.output.get(k),
        }
    };
    let truthy = |v: Option<&OutValue>| !matches!(v, None | Some(OutValue::Bool(false)));
    let main_hand = match own {
        Some(o) => o.main_hand.as_ref(),
        None => state.player.hand_output(0),
    };
    enemy_mods(env, x);
    // :3869-3878
    for dt in ["Physical", "Lightning", "Cold", "Fire", "Chaos"] {
        let name = format!("Enemy{dt}ResistEqualToYours");
        let key = format!("{dt}Resist");
        if flag(&env.mod_db, &[&name]) && truthy(output(&key)) {
            x.player_mods.insert(name);
            x.player_mods.insert(format!("{key}={}", tostring_out(output(&key))));
        }
    }
    if flag(&env.mod_db, &["PartyMemberMaximumEnduranceChargesEqualToYours"]) && truthy(output("EnduranceChargesMax")) {
        x.player_mods.insert("PartyMemberMaximumEnduranceChargesEqualToYours".to_owned());
        x.player_mods
            .insert(format!("EnduranceChargesMax={}", tostring_out(output("EnduranceChargesMax"))));
    }
    // :3880
    let speed = match output("MovementSpeedMod") {
        Some(OutValue::Num(v)) => *v,
        other => panic!("{CP}:3880: attempt to perform arithmetic on field 'MovementSpeedMod' ({other:?})"),
    };
    x.player_mods
        .insert(format!("MovementSpeedMod|percent|max={}", tostring_num(speed * 100.0)));
    // :3882-3891
    if x.aura["extraAura"].mods.iter().any(|m| &*m.name == "YourFortifyEqualToParent") {
        x.player_mods.insert("PartyMemberFortifyEqualToYours".to_owned());
        let stacks = match output("FortificationStacks") {
            v if truthy(v) => tostring_out(v),
            _ => "0".to_owned(),
        };
        x.player_mods.insert(format!("FortificationStacks={stacks}"));
    }
    // :3893-3931 preStack Mine auras
    let names: Vec<String> = x
        .aura
        .keys()
        .filter(|k| k.contains("Mine") && !k.contains(" Limit"))
        .cloned()
        .collect();
    for name in names {
        let stack_key = format!("Multiplier:{name}Stack");
        // `X and X.value or 0`; a `MultiStat` table has no `value`
        let stack = match x.enemy_mods.remove(&stack_key) {
            Some(EnemyExport::One(m)) if m.value.truthy() => m.value.clone(),
            _ => Value::Num(0.0),
        };
        let none = stack.as_num() == Some(0.0);
        for key in [name.clone(), format!("{name} Limit")] {
            if none {
                x.aura.remove(&key);
            } else if let Some(entry) = x.aura.get_mut(&key) {
                pre_stack(entry, &stack);
            }
        }
    }
    // :3933-3956
    let links: Vec<(String, Vec<Rc<Mod>>)> = x.link.iter().map(|(k, l)| (k.clone(), l.mods.clone())).collect();
    for (link_name, mods) in links {
        if link_name == "Flame Link" {
            x.player_mods.insert(format!("Life={}", tostring_out(output("Life"))));
        }
        for m in &mods {
            // `mod.name:match("Parent")` first: :3943 "MaximumLifeLeechIsEqualToPartyMember" has no "Parent" and never comes here
            match &*m.name {
                "MaximumBlockAttackChanceIsEqualToParent" => {
                    x.player_mods
                        .insert(format!("BlockChanceMax={}", tostring_out(output("BlockChanceMax"))));
                }
                "BlockAttackChanceIsEqualToParent" => {
                    x.player_mods.insert(format!("BlockChance={}", tostring_out(output("BlockChance"))));
                }
                "TakenFromParentESBeforeYou" => {
                    x.player_mods.insert(format!(
                        "EnergyShieldRecoveryCap={}",
                        tostring_out(output("EnergyShieldRecoveryCap"))
                    ));
                }
                "MainHandCritIsEqualToParent" => {
                    let crit = match main_hand {
                        Some(h) => tostring_out(h.get("CritChance")),
                        // :3950 `env.player.weaponData1` is always a table (CalcSetup.lua)
                        None => format_value(&env.player.borrow().weapon_data1.get("CritChance")),
                    };
                    x.player_mods.insert(format!("MainHand.CritChance={crit}"));
                }
                _ => {}
            }
        }
    }
}

/// :3839-3867 `pairs(enemyDB.mods)`.
fn enemy_mods(env: &Env, x: &mut BuffExports) {
    let db = env.enemy_db.borrow();
    let StoreKind::Db(d) = &db.kind else {
        panic!("{CP}:3839: enemyDB is not a ModDB")
    };
    // `enemyDB.mods[key] and enemyDB.mods[key][1].value`
    let first_value = |key: &str| d.mods.get(key).is_some_and(|l| l.first().is_some_and(|m| m.value.truthy()));
    let var_of = |t: &Tag, line: u32| -> String {
        match t.get("var") {
            Value::Str(s) => s.to_string(),
            other => panic!("{CP}:{line}: attempt to concatenate field 'var' (a {} value)", other.type_name()),
        }
    };
    for (k, mods) in &d.mods {
        let k: &str = k;
        if k.contains("Condition") && !k.contains("Party") {
            x.enemy_conditions.insert(k.to_owned());
            continue;
        }
        let wanted = (k.contains("Resist") && !k.contains("Totem") && !k.contains("Max"))
            || k.contains("Damage")
            || k.contains("ActionSpeed")
            || k.contains("SelfCrit")
            || (k.contains("Multiplier") && !k.contains("Max") && !k.contains("Impale"));
        if !wanted {
            continue;
        }
        for v in mods {
            let source = v.source.as_deref();
            if v.value.as_num() == Some(0.0) || source == Some("EnemyConfig") || source == Some("Base") {
                continue;
            }
            let source = source.unwrap_or_else(|| panic!("{CP}:3844: attempt to index field 'source' (a nil value)"));
            if source.contains("Delirious") || source.starts_with("Party") {
                continue;
            }
            // :3845-3851
            let skip = v
                .tags
                .iter()
                .any(|t| matches!(t.get("effectType").as_str(), Some("Curse" | "AuraDebuff")));
            if skip {
                continue;
            }
            // :3852-3854
            let keep = match v.tags.first() {
                None => true,
                Some(t) => {
                    let ty = t.get("type");
                    let ty = ty.as_str();
                    let condition = ty != Some("Condition")
                        || (t.get("var").truthy() && first_value(&format!("Condition:{}", var_of(t, 3853))))
                        || match t.get("varList") {
                            Value::Table(l) => match l.array.first() {
                                Some(Value::Str(s)) => first_value(&format!("Condition:{s}")),
                                _ => panic!("{CP}:3853: attempt to concatenate field '?' (a non-string value)"),
                            },
                            _ => false,
                        };
                    condition && (ty != Some("Multiplier") || first_value(&format!("Multiplier:{}", var_of(t, 3854))))
                }
            };
            if !keep {
                continue;
            }
            // :3855-3862
            match x.enemy_mods.remove(k) {
                None => {
                    x.enemy_mods.insert(k.to_owned(), EnemyExport::One(v.clone()));
                }
                Some(EnemyExport::One(first)) => {
                    x.enemy_mods.insert(k.to_owned(), EnemyExport::Multi(vec![first, v.clone()]));
                }
                Some(EnemyExport::Multi(mut l)) => {
                    l.push(v.clone());
                    x.enemy_mods.insert(k.to_owned(), EnemyExport::Multi(l));
                }
            }
        }
    }
}
