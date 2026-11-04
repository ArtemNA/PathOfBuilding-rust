//! Readers for `Classes/CompareTab.lua`: `calcFunc` / `getMiscCalculator` of the primary build read through `CalculatePowerStat` (`ComparePowerBuilder`, :2516-3126), the groups of an entry
//! as models, the `if*` predicates of `Modules/ConfigVisibility.lua`. Nothing here writes to the engine's state.

use std::rc::Rc;

use pob_calc::env::CalcOverride;
use pob_calc::pob_build::{self, BuildModel, SocketGroupModel};
use pob_calc::power::{PowerStat, get_from_output};

use crate::calcs::{CalcsActor, Entry};
use crate::engine::Engine;
use pob_calc::breakdown::BVal;

/// A row of `Tabulate` or of a `modList`: `row.value`, `row.mod.name/.type/.source`.
#[derive(Clone, Debug)]
pub struct ModRowData {
    pub value: pob_mods::Value,
    pub name: String,
    pub ty: String,
    pub source: Option<String>,
}

pub type BuffRow = ModRowData;

fn row_of(m: &pob_mods::Mod, value: pob_mods::Value) -> ModRowData {
    ModRowData {
        value,
        name: m.name.to_string(),
        ty: m.ty.map_or_else(String::new, |t| t.as_str().to_string()),
        source: m.source.as_deref().map(str::to_string),
    }
}

/// `TabulateMods` + `GetModStoreAndCfg` (CompareCalcsHelpers.lua:79-110).
pub fn tabulate_entry(sd: &Entry, actor: &CalcsActor<'_>) -> Vec<ModRowData> {
    let mut cfg = match sd.cfg.as_deref() {
        Some(name) if actor.skill_mod_list.is_some() => (actor.cfg)(name).unwrap_or_default(),
        _ => pob_mods::ModCfg::default(),
    };
    cfg.source = sd.mod_source.as_deref().map(pob_mods::Str::from);
    cfg.actor = sd.actor.as_deref().map(pob_mods::Str::from);
    let store = match (&sd.enemy, &actor.enemy_db, &sd.cfg, &actor.skill_mod_list) {
        (Some(true), Some(db), _, _) => db.clone(),
        (_, _, Some(_), Some(l)) => l.clone(),
        _ => actor.mod_db.clone(),
    };
    let ty = sd.mod_type.as_deref().and_then(|t| crate::calcs_tab::mod_type(t).ok());
    let names: Vec<&str> = sd.mod_name.iter().map(String::as_str).collect();
    let rows = store.borrow().tabulate(ty, Some(&cfg), &names).unwrap_or_default();
    rows.into_iter().map(|e| row_of(&e.mod_, e.value)).collect()
}

impl Engine {
    /// `data.powerStatList.GetFromOutput(calcBase, powerStat)` (:2624).
    pub fn compare_base_stat(&self, stat: &PowerStat) -> Result<f64, String> {
        let calc = self.misc_calculator()?;
        let r = calc.base.borrow();
        Ok(get_from_output(&r.state.player.output, stat))
    }

    /// `CalculatePowerStat(powerStat, calcFunc(override, useFullDPS), calcBase)` with `useFullDPS = powerStat.stat == "FullDPS"` (:2518, :2673-2676).
    pub fn compare_override_impact(&self, ov: CalcOverride, stat: &PowerStat) -> Result<f64, String> {
        let calc = self.misc_calculator()?;
        let use_full = stat.stat.as_deref() == Some("FullDPS");
        let run = calc
            .calc(&Rc::new(ov), Some(use_full), self.view_mode_is_tree.get())
            .map_err(|e| e.0)?;
        let value = get_from_output(&run.borrow().state.player.output, stat);
        let base = get_from_output(&calc.base.borrow().state.player.output, stat);
        Ok(value - base)
    }

    /// `CalculatePowerStat(powerStat, gemCalcBase, calcBase)` where the first is the base of `getMiscCalculator` over the build with a temporary edit (:2916-2932, :3000-3014, :3067-3085):
    /// the model is cloned, the engine's state stays.
    pub fn compare_edited_impact(&self, edit: impl FnOnce(&mut BuildModel), stat: &PowerStat) -> Result<f64, String> {
        let mut m = self.model().clone();
        edit(&mut m);
        let sess = self.calc_session(m);
        crate::tree_view::main_pass_for_calculator(&sess)?;
        let calc = sess.get_misc_calculator().map_err(|e| e.0)?;
        let value = get_from_output(&calc.base.borrow().state.player.output, stat);
        drop(calc);
        Ok(value - self.compare_base_stat(stat)?)
    }

    /// :3064-3085: `input[var] = value`, `BuildModList`, the base of a new `getMiscCalculator`, the value put back, `BuildModList`: all over the `GlobalCache` of the calculator (no wipe), so
    /// a skill that reads the cache (a mirage) lags one step behind the config, as in the Lua. The engine's tables stay.
    pub fn compare_config_impact(&self, var: &str, value: Option<pob_build::InputValue>, stat: &PowerStat) -> Result<f64, String> {
        let level = self.model().level;
        let mut st = self.config_state().clone();
        if let Some(c) = &self.config_controls {
            st.controls = c.clone();
        }
        let saved = st.input.get(var).cloned();
        let write = |st: &mut pob_build::ConfigState, v: Option<pob_build::InputValue>| match v {
            Some(v) => {
                st.input.insert(var.to_owned(), v);
            }
            None => {
                st.input.shift_remove(var);
            }
        };
        let calc = self.misc_calculator()?;
        write(&mut st, value);
        let lists = st.build_mod_list(level).map_err(|e| e.to_string())?;
        calc.session().set_config(st.clone(), lists).map_err(|e| e.0)?;
        let edited = calc.session().get_misc_calculator().map_err(|e| e.0)?;
        let v = get_from_output(&edited.base.borrow().state.player.output, stat);
        drop(edited);
        write(&mut st, saved);
        let lists = st.build_mod_list(level).map_err(|e| e.to_string())?;
        calc.session().set_config(st.clone(), lists).map_err(|e| e.0)?;
        Ok(v - get_from_output(&calc.base.borrow().state.player.output, stat))
    }

    /// :3998-4028: `getMiscCalculator` made anew at every hover, `calcFunc(override)`, `AddStatComparesToTooltip`: the base is of the live slots, not of the last `BuildOutput`.
    pub fn compare_fresh_lines(&self, ov: CalcOverride, header: &str) -> Result<Vec<String>, String> {
        let sess = self.calc_session(self.model().clone());
        crate::tree_view::main_pass_for_calculator(&sess)?;
        let calc = sess.get_misc_calculator().map_err(|e| e.0)?;
        let run = calc.calc(&Rc::new(ov), None, self.view_mode_is_tree.get()).map_err(|e| e.0)?;
        let mut lines = Vec::new();
        self.add_stat_compares(&mut lines, &calc.base, &run, header, None)?;
        Ok(lines)
    }

    /// `GetBreakdownLines` (CompareCalcsHelpers.lua:145-167) over `calcsEnv.player.breakdown`.
    pub fn compare_breakdown_lines(&self, key: &str) -> Option<Vec<String>> {
        let bd = self.calcs_run().state.player.breakdown.as_ref()?;
        let simple = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
        let v = match key.split_once('.') {
            Some((ns, name)) if simple(ns) && simple(name) => match bd.get(ns)? {
                BVal::Table(t) => t.fields.get(name).cloned(),
                BVal::Module(m) => m.get(name).cloned(),
                _ => None,
            },
            _ => bd.get(key).cloned(),
        }?;
        let BVal::Table(t) = v else { return None };
        // `#breakdown == 0`
        if t.list.is_empty() {
            return None;
        }
        let lines: Vec<String> = t
            .list
            .iter()
            .filter_map(|l| match l {
                BVal::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        (!lines.is_empty()).then_some(lines)
    }

    /// `player.breakdown[key].modList` (`SkillBuffs` / `SkillDebuffs`, CompareCalcsHelpers.lua:365-372).
    pub fn compare_breakdown_mods(&self, key: &str) -> Option<Vec<BuffRow>> {
        let bd = self.calcs_run().state.player.breakdown.as_ref()?;
        let BVal::Table(t) = bd.get(key)? else { return None };
        let BVal::Table(list) = t.fields.get("modList")? else { return None };
        Some(
            list.list
                .iter()
                .filter_map(|r| match r {
                    BVal::Table(t) => match t.fields.get("mod") {
                        Some(BVal::Mod(m)) => {
                            let value = match t.fields.get("value") {
                                Some(BVal::Num(n)) => pob_mods::Value::Num(*n),
                                Some(BVal::Str(s)) => pob_mods::Value::Str(s.as_str().into()),
                                Some(BVal::Bool(b)) => pob_mods::Value::Bool(*b),
                                _ => m.0.value.clone(),
                            };
                            Some(row_of(&m.0, value))
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect(),
        )
    }

    /// `build.data.skills[id].name`.
    pub fn compare_skill_name(&self, id: &str) -> Option<String> {
        self.skill_data().get(id).map(|s| s.name.clone())
    }

    /// `CompareEntry:RefreshSkillSelectControls` (CompareEntry.lua:289-377): `Build`'s over the same run, but with its own default of the stages:
    /// `skillStageCount or selectedPart.stagesMin or 1` (:357), `skillStageCount or skillData.stagesMin or 1` (:370).
    pub fn compare_skill_select(&self, calcs: bool) -> Result<crate::shell::SkillSelect, String> {
        let mut sel = if calcs { self.skill_select_calcs()? } else { self.skill_select()? };
        let env = if calcs { &self.calcs_run().env } else { &self.result().env };
        let Some(m) = sel.main.as_mut() else { return Ok(sel) };
        if m.stage_count.is_none() {
            return Ok(sel);
        }
        let Some(st) = env.skills.as_ref() else { return Ok(sel) };
        let list = st.tab.socket_group_list();
        let Some(group) = list.get((sel.main_group as usize).wrapping_sub(1)) else {
            return Ok(sel);
        };
        let Some(display) = group.display_skill_list.as_ref() else {
            return Ok(sel);
        };
        let Some(si) = display.get((m.skill_sel as usize).wrapping_sub(1)) else {
            return Ok(sel);
        };
        let active = st.skill(*si);
        let ae = active.active_effect.borrow();
        let ge = &st.skills.effects[ae.granted_effect];
        let src = ae.src_instance.and_then(|(g, k)| list.get(g).and_then(|g| g.gem_list.get(k)));
        let own = src.and_then(|s| if calcs { s.skill_stage_count_calcs } else { s.skill_stage_count });
        let multi_part = ge.parts.as_ref().is_some_and(|p| p.len() > 1);
        // `stagesMin` is not a field of any part in the data (shell.rs)
        let default = if multi_part {
            None
        } else {
            active.skill_data.get("stagesMin").and_then(pob_mods::Value::as_num)
        };
        m.stage_count = Some(pob_lua::tostring_num(own.or(default).unwrap_or(1.0)));
        Ok(sel)
    }

    /// `GetGemGrantedEffect(gem)` (:2468-2473): name and `support`.
    pub fn compare_gem_effect(&self, gem: &pob_calc::skills_tab::GemInstance) -> Option<(String, bool)> {
        let skills = self.skill_data();
        let id = gem
            .gem_data
            .as_ref()
            .and_then(|id| pob_data::gems::gems().gems.0.get(id))
            .map(|d| &d.granted_effect_id)
            .or(gem.granted_effect.as_ref())?;
        let ge = skills.get(id)?;
        Some((ge.name.clone(), ge.support))
    }

    /// `getImbuedGem` (:4066-4070): `imbuedSupportBySlot[group.slot]`, else `data.gemForBaseName`; `None` without `group.imbuedSupport`. The name and `color` of the effect.
    pub fn compare_imbued_effect(&self, group: &pob_calc::skills_tab::SocketGroup) -> Option<(Option<String>, Option<f64>)> {
        let imbued = group.imbued_support.as_ref()?;
        let skills = self.skill_data();
        let data = pob_data::gems::gems();
        let id = group
            .slot
            .as_ref()
            .and_then(|s| self.skills_tab().imbued_support_by_slot.get(s).cloned())
            .or_else(|| {
                let key = format!("{} support", imbued.to_ascii_lowercase());
                data.gem_for_base_name
                    .get(&key)
                    .and_then(|g| data.gems.get(g))
                    .map(|g| g.granted_effect_id.clone())
            });
        let ge = id.as_ref().and_then(|i| skills.get(i));
        Some((ge.map(|g| g.name.clone()), ge.and_then(|g| g.color)))
    }

    /// `checkSupporting(gemA, gemB)` (:4176-4185) over two gems with `gemData`; `b_at`: `(group, gem)` of B.
    pub fn compare_is_supporting(
        &self,
        a: &pob_calc::skills_tab::GemInstance,
        b: &pob_calc::skills_tab::GemInstance,
        b_at: (usize, usize),
    ) -> bool {
        let skills = self.skill_data();
        let data = pob_data::gems::gems();
        let (Some(ga), Some(gb)) = (
            a.gem_data.as_ref().and_then(|i| data.gems.get(i)),
            b.gem_data.as_ref().and_then(|i| data.gems.get(i)),
        ) else {
            return false;
        };
        let a_support = skills.get(&ga.granted_effect_id).is_some_and(|e| e.support)
            || ga
                .secondary_granted_effect_id
                .as_ref()
                .and_then(|s| skills.get(s))
                .is_some_and(|e| e.support);
        a_support
            && !skills.get(&gb.granted_effect_id).is_some_and(|e| e.support)
            && a.support_effect
                .as_ref()
                .is_some_and(|s| s.borrow().is_supporting.as_ref().is_some_and(|l| l.contains(&b_at)))
    }

    /// `socketGroupList` of the active set as the models a next load would read: with the groups CalcSetup added for item-granted skills.
    pub fn compare_group_models(&self) -> Result<Vec<SocketGroupModel>, String> {
        let mut m = pob_build::load_build_xml(&self.save_xml()?).map_err(|e| format!("XML: {e}"))?;
        Ok(crate::engine::active_skill_set_mut(&mut m).map_or_else(Vec::new, |s| s.groups.clone()))
    }
}

/// `skillSets[activeSkillSetId].socketGroupList` of a model to edit for `compare_edited_impact`.
pub fn model_groups_mut(m: &mut BuildModel) -> Option<&mut Vec<SocketGroupModel>> {
    crate::engine::active_skill_set_mut(m).map(|s| &mut s.groups)
}
