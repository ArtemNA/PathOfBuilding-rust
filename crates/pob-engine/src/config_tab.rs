//! The data half of `Classes/ConfigTab.lua` for the tab (U7): `configSets[activeConfigSetId]` lives on as the MAIN run's `env.config` ([`Engine::config_state`]), edited in place
//! the way the Lua edits its tables -- a callback writes `input[var]`, runs `BuildModList` at once (which writes placeholders, `enemyDamageType`, the controls' `enabled`), and
//! leaves the recalculation to the next frame (`buildFlag`). Every such edit is also written to the model's set, which is what the next run and `SaveDB` read.
//!
//! The `if*` gates of the tab over `mainEnv` (ConfigTab.lua:382-662): [`Engine::config_gate`].

use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_calc::env::CalcOverride;
use pob_calc::env_used::{EnvUsed, EnvUsedMods, env_used, env_used_mods};
use pob_calc::pob_build::{BuildModel, ConfigSetModel, ConfigState, CustomModsBlockModel, InputValue};

use crate::engine::Engine;

/// Which `mainEnv` table an `if*` key reads (ConfigTab.lua:382-662).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigGate {
    Cond,
    MinionCond,
    EnemyCond,
    CondTrue,
    Mult,
    EnemyMult,
    Stat,
    TagType,
    Flag,
    Mod,
    Skill,
    SkillTransfigured,
    SkillFlag,
    SkillData,
}

const GATE_COUNT: usize = 14;

/// What the gates answered over the current run: `mainEnv` does not change until the next one. The tab asks for every control on every frame (as the Lua does).
#[derive(Default)]
pub struct ConfigGates {
    used: Option<EnvUsed>,
    /// The mods of the tables, for the dev-mode tooltip only.
    mods: Option<EnvUsedMods>,
    memo: [HashMap<String, bool>; GATE_COUNT],
}

/// The comparison lines of a tooltip and the `SelByValue`s its two `BuildModList`s did.
pub type ConfigCompare = (Vec<String>, Vec<(String, usize)>);

/// `ConfigTab:CreateUndoState` (:1242-1248).
#[derive(Clone)]
pub struct ConfigUndo {
    pub input: IndexMap<String, InputValue>,
    pub custom_mods_list: Vec<CustomModsBlockModel>,
}

/// One active set as `from_set` replays it: the state's tables, with a nil write for each default the state does not have.
fn write_set(set: &mut ConfigSetModel, st: &ConfigState) {
    let fresh = ConfigState::new();
    set.input.clear();
    for k in fresh.input.keys() {
        if !st.input.contains_key(k) {
            set.input.insert(k.clone(), None);
        }
    }
    for (k, v) in &st.input {
        set.input.insert(k.clone(), Some(v.clone()));
    }
    set.placeholder.clear();
    for k in fresh.placeholder.keys() {
        if !st.placeholder.contains_key(k) {
            set.placeholder.insert(k.clone(), None);
        }
    }
    for (k, v) in &st.placeholder {
        if let InputValue::Num(n) = v {
            set.placeholder.insert(k.clone(), Some(*n));
        }
    }
    set.custom_mods_list.clone_from(&st.custom_mods_list);
}

fn active_set_mut(m: &mut BuildModel) -> Option<&mut ConfigSetModel> {
    let active = m.config.active_config_set;
    m.config.sets.iter_mut().find(|s| s.id == active)
}

/// `ipairs(configSetOrderList)` of the model's sparse pairs.
fn order_of(m: &BuildModel) -> Vec<f64> {
    let mut order = m.config.set_order_list.clone();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::with_capacity(order.len());
    for (i, (index, id)) in order.iter().enumerate() {
        if *index != (i + 1) as f64 {
            break;
        }
        out.push(*id);
    }
    out
}

impl Engine {
    /// `configSets[activeConfigSetId]` + `enemyLevel`.
    pub fn config_state(&self) -> &ConfigState {
        &self.result().env.config
    }

    fn sync_config_to_model(&mut self) {
        let st = self.result().env.config.clone();
        if let Some(set) = active_set_mut(self.model_mut()) {
            write_set(set, &st);
            // a new build has no `<Config>`: what the next run reads is this set from now on, or `ConfigState::from_build` would start from the defaults
            self.model_mut().has_config_section = true;
        }
    }

    /// `input[var] = v` (:299, :309, :317); nil deletes.
    pub fn config_write_input(&mut self, var: &str, v: Option<InputValue>) {
        let c = &mut self.res_mut().env.config;
        match v {
            Some(v) => {
                c.input.insert(var.to_owned(), v);
            }
            None => {
                c.input.shift_remove(var);
            }
        }
        self.sync_config_to_model();
    }

    /// The placeholder part of `UpdateControls` (:1011-1014), over the controls `BuildModList` reads next: without it the apply closures of a set just switched to read what the previous set left.
    pub fn config_update_control_placeholders(&mut self) {
        let controls = self.config_controls.take();
        let carried = controls.is_some();
        let c = &mut self.res_mut().env.config;
        if let Some(ctl) = controls {
            c.controls = ctl;
        }
        c.update_control_placeholders();
        if carried {
            self.config_controls = Some(self.result().env.config.controls.clone());
        }
    }

    /// `ConfigTab:BuildModList()` (:1130-1203): its lists are rebuilt by the next run; here its writes to the set and to the controls. Returns the `SelByValue`s it did on the
    /// dropdowns (1-based), the `selIndex` of the tab's controls from here on.
    pub fn config_build_mod_list(&mut self) -> Result<Vec<(String, usize)>, String> {
        let level = self.model().level;
        let controls = self.config_controls.take();
        let c = &mut self.res_mut().env.config;
        if let Some(ctl) = controls {
            c.controls = ctl;
        }
        c.build_mod_list(level).map_err(|e| e.to_string())?;
        let sel: Vec<(String, usize)> = c.controls.sel_index.drain(..).collect();
        self.config_controls = Some(self.result().env.config.controls.clone());
        self.sync_config_to_model();
        Ok(sel)
    }

    /// `varControls[var].enabled` as `BuildModList` left it.
    pub fn config_control_enabled(&self, var: &str) -> bool {
        match &self.config_controls {
            Some(c) => c.enabled.get(var).copied().unwrap_or(true),
            None => self.config_state().control_enabled(var),
        }
    }

    /// `ConfigTab:GetDefaultState(var, type(cur))` (:941-959).
    pub fn config_default_state(&self, var: &str, cur: &InputValue) -> InputValue {
        crate::save::get_default_state(&self.config_state().placeholder, var, cur)
    }

    /// `GetDefaultState(var)` (`varType` nil).
    pub fn config_default_state_nil(&self, var: &str) -> Option<InputValue> {
        self.config_state()
            .placeholder
            .get(var)
            .or_else(|| crate::save::default_state().get(var))
            .cloned()
    }

    pub fn config_active_set(&self) -> f64 {
        self.model().config.active_config_set
    }

    /// `configSetOrderList`.
    pub fn config_set_order(&self) -> Vec<f64> {
        order_of(self.model())
    }

    /// `configSets[id].title`.
    pub fn config_set_title(&self, id: f64) -> Option<String> {
        self.model().config.sets.iter().find(|s| s.id == id).and_then(|s| s.title.clone())
    }

    /// `configSets[id] ~= nil`.
    pub fn config_set_exists(&self, id: f64) -> bool {
        self.model().config.sets.iter().any(|s| s.id == id)
    }

    /// `#configSetOrderList` replaced (the list the manage popup edits).
    pub fn config_write_order(&mut self, order: &[f64]) {
        self.model_mut().config.set_order_list = order.iter().enumerate().map(|(i, id)| ((i + 1) as f64, *id)).collect();
    }

    /// The table part of `SetActiveConfigSet` (:1373-1400): an unknown id falls to the first of the list. `UpdateControls` + `BuildModList` are the tab's.
    pub fn config_activate(&mut self, id: f64) {
        // :1375-1378
        if order_of(self.model()).is_empty() {
            let m = self.model_mut();
            m.config.set_order_list.retain(|(i, _)| *i != 1.0);
            m.config.set_order_list.push((1.0, 1.0));
            m.config.set_order_list.sort_by(|a, b| a.0.total_cmp(&b.0));
            self.config_create_set(Some(1.0), None);
        }
        let id = if self.config_set_exists(id) {
            id
        } else {
            order_of(self.model())[0]
        };
        self.sync_config_to_model();
        self.model_mut().config.active_config_set = id;
        let set = self
            .model()
            .config
            .sets
            .iter()
            .find(|s| s.id == id)
            .expect("configSets[id]")
            .clone();
        let enemy_level = self.config_state().enemy_level;
        let controls = self.config_state().controls.clone();
        let c = &mut self.res_mut().env.config;
        *c = ConfigState::from_set(&set);
        c.enemy_level = enemy_level;
        c.controls = controls;
    }

    /// `ConfigTab:CreateConfigSet(configSetId, title)` (ConfigTab.lua:1282-1299): without an id `#self.configSets + 1`; the defaults of
    /// `input` / `placeholder` are `ConfigState::from_set`'s.
    fn config_create_set(&mut self, id: Option<f64>, title: Option<&str>) -> f64 {
        let id = id.unwrap_or_else(|| crate::sets::next_id(&self.set_tables.config));
        self.set_tables.config.set(id);
        let set = ConfigSetModel {
            id,
            title: title.map(str::to_owned),
            input: IndexMap::new(),
            placeholder: IndexMap::new(),
            custom_mods_list: vec![CustomModsBlockModel::default_block()],
        };
        let m = self.model_mut();
        match m.config.sets.iter_mut().find(|s| s.id == id) {
            Some(s) => *s = set,
            None => m.config.sets.push(set),
        }
        id
    }

    /// `t_insert(self.configSetOrderList, id)`.
    fn config_order_push(&mut self, id: f64) {
        let m = self.model_mut();
        let n = order_of(m).len();
        m.config.set_order_list.retain(|(i, _)| *i != (n + 1) as f64);
        m.config.set_order_list.push(((n + 1) as f64, id));
        m.config.set_order_list.sort_by(|a, b| a.0.total_cmp(&b.0));
    }

    /// `ConfigTab:NewConfigSet(configSetId, title)` (ConfigTab.lua:1302-1307): `CreateConfigSet` + the end of the order. The id.
    pub fn config_new_set(&mut self, id: Option<f64>, title: Option<&str>) -> f64 {
        let id = self.config_create_set(id, title);
        self.config_order_push(id);
        id
    }

    /// `#self.configSets`.
    pub fn config_sets_len(&self) -> usize {
        self.set_tables.config.len()
    }

    /// `ConfigTab:CopyConfigSet(configSetId, newConfigSetName)` (ConfigTab.lua:1309-1318): `copyTable` with the id `#self.configSets + 1`,
    /// `t_insert(self.configSets, newConfigSet)` (the same key: nothing changed `#t` in between), the end of the order. The id.
    pub fn config_copy_set(&mut self, from: f64, title: &str) -> f64 {
        let id = crate::sets::next_id(&self.set_tables.config);
        let mut s = self
            .model()
            .config
            .sets
            .iter()
            .find(|s| s.id == from)
            .expect("Classes/ConfigTab.lua:1311: attempt to index local 'configSet' (a nil value)")
            .clone();
        s.id = id;
        s.title = Some(title.to_owned());
        self.set_tables.config.set(id);
        self.model_mut().config.sets.push(s);
        self.config_order_push(id);
        id
    }

    /// `ConfigTab:RenameConfigSet` (ConfigTab.lua:1320-1329): a gone id does nothing.
    pub fn config_rename_set(&mut self, id: f64, title: &str) {
        if let Some(s) = self.model_mut().config.sets.iter_mut().find(|s| s.id == id) {
            s.title = Some(title.to_owned());
        }
    }

    /// `ConfigTab:DeleteConfigSet(configSetId, orderListIndex)` (ConfigTab.lua:1332-1336): `t_remove(self.configSetOrderList, orderListIndex)`,
    /// `configSets[id] = nil`.
    pub fn config_delete_set(&mut self, id: f64, order_index: usize) {
        let mut order = order_of(self.model());
        assert!(
            order.get(order_index - 1) == Some(&id),
            "ConfigTab:DeleteConfigSet: set {id} is not at {order_index}"
        );
        order.remove(order_index - 1);
        self.config_write_order(&order);
        self.model_mut().config.sets.retain(|s| s.id != id);
        crate::sets::remove(&mut self.set_tables.config, id);
    }

    /// `customModsList` of the active set replaced.
    pub fn config_write_custom_mods(&mut self, list: Vec<CustomModsBlockModel>) {
        self.res_mut().env.config.custom_mods_list = list;
        self.sync_config_to_model();
    }

    /// `ConfigTab:CreateUndoState` (:1242-1248).
    pub fn config_undo_state(&self) -> ConfigUndo {
        let c = self.config_state();
        ConfigUndo {
            input: c.input.clone(),
            custom_mods_list: c.custom_mods_list.clone(),
        }
    }

    /// The table part of `RestoreUndoState` (:1250-1268).
    pub fn config_restore_undo(&mut self, s: ConfigUndo) {
        let c = &mut self.res_mut().env.config;
        c.input = s.input;
        c.custom_mods_list = s.custom_mods_list;
        self.sync_config_to_model();
    }

    /// ConfigTab.lua:776-790 and :67-73: `edit` the set, `BuildModList`, `calcFunc()`, `restore` the set, `BuildModList`. The Lua works on its live tables, so what the two
    /// `BuildModList`s leave outside the edited field stays (`presetBossSkills` writes `input.enemyDamageType`); `buildFlag` is put back. Returns the lines and the `SelByValue`s.
    pub fn config_compare(
        &mut self,
        edit: impl FnOnce(&mut ConfigState),
        restore: impl FnOnce(&mut ConfigState),
        header: &str,
    ) -> Result<ConfigCompare, String> {
        // `self.calcFunc` and `self.calcBase` are of the last rebuild, before these writes: the same GlobalCache as its base pass
        let level = self.model().level;
        let mut st = self.config_state().clone();
        if let Some(c) = &self.config_controls {
            st.controls = c.clone();
        }
        let view_tree = self.view_mode_is_tree.get();
        let lines = {
            let calc = self.misc_calculator()?;
            edit(&mut st);
            let lists = st.build_mod_list(level).map_err(|e| e.to_string())?;
            calc.session().set_config(st.clone(), lists).map_err(|e| e.0)?;
            let run = calc.calc(&Rc::new(CalcOverride::default()), None, view_tree).map_err(|e| e.0)?;
            let mut lines = Vec::new();
            self.add_stat_compares(&mut lines, &calc.base, &run, header, None)?;
            restore(&mut st);
            let lists = st.build_mod_list(level).map_err(|e| e.to_string())?;
            calc.session().set_config(st.clone(), lists).map_err(|e| e.0)?;
            lines
        };
        let sel: Vec<(String, usize)> = st.controls.sel_index.drain(..).collect();
        self.config_controls = Some(st.controls.clone());
        self.res_mut().env.config = st;
        self.sync_config_to_model();
        Ok((lines, sel))
    }

    /// `build.viewMode`, what `SaveDB` writes.
    pub fn set_view_mode(&mut self, mode: &str) {
        self.model_mut().view_mode = Some(mode.to_owned());
    }

    /// `controls.edit.buf` of the NotesTab, what `Save` writes.
    pub fn set_notes(&mut self, text: &str) {
        self.extras_mut().notes = text.to_owned();
    }

    pub fn notes(&self) -> &str {
        &self.extras().notes
    }

    /// `mainOutput[stat]` as a number (the two tooltip functions, ConfigOptions.lua:61-72).
    pub fn main_output_num(&self, stat: &str) -> f64 {
        self.result().perform.player.output.num(stat)
    }

    /// The `launch.devModeAlt` part of one `if*` tooltip (ConfigTab.lua:408-622), for one name: nil when the table has no such name. The mods are collected only here, once per run.
    pub fn config_dev_tooltip(&self, kind: ConfigGate, opt: &str) -> Option<String> {
        use pob_calc::env_used::UsedTable as T;
        let env = &self.result().env;
        let tables: &[T] = match kind {
            ConfigGate::Cond => &[T::Conditions],
            ConfigGate::MinionCond => &[T::MinionConditions],
            ConfigGate::EnemyCond => &[T::EnemyConditions],
            ConfigGate::Mult => &[T::Multipliers],
            ConfigGate::EnemyMult => &[T::EnemyMultipliers],
            // :535-546
            ConfigGate::Stat => &[T::PerStats, T::EnemyMultipliers],
            ConfigGate::TagType => &[T::TagTypes],
            ConfigGate::Mod => &[T::Mods],
            // :471-477
            ConfigGate::CondTrue => {
                let db = env.player.borrow().mod_db.clone();
                let d = db.borrow();
                let state = d.conditions.get(opt).map_or("nil", |v| if *v { "true" } else { "false" });
                return Some(format!("Condition state: {opt}={state}"));
            }
            _ => return None,
        };
        let mut g = self.config_gates.borrow_mut();
        let mods = g.mods.get_or_insert_with(|| env_used_mods(env));
        let mut out: Option<String> = None;
        for table in tables {
            for m in mods.get(*table, opt).unwrap_or_default() {
                let source = m
                    .source
                    .as_deref()
                    .expect("Classes/ConfigTab.lua:420: attempt to concatenate field 'source' (a nil value)");
                let text = format!("{}|{source}", pob_mods::format::format_mod(m));
                out = Some(match out {
                    Some(o) => format!("{o}\n{text}"),
                    None => text,
                });
            }
        }
        out
    }

    /// One `if*` test over `mainEnv` (ConfigTab.lua:382-662), for one name.
    pub fn config_gate(&self, kind: ConfigGate, opt: &str) -> bool {
        let mut g = self.config_gates.borrow_mut();
        if let Some(a) = g.memo[kind as usize].get(opt) {
            return *a;
        }
        let env = &self.result().env;
        if g.used.is_none() {
            g.used = Some(env_used(env));
        }
        let used = g.used.as_ref().expect("just made");
        let st = env.skills.as_ref();
        let answer = match kind {
            ConfigGate::Cond => used.conditions.contains(opt),
            ConfigGate::MinionCond => used.minion_conditions.contains(opt),
            ConfigGate::EnemyCond => used.enemy_conditions.contains(opt),
            ConfigGate::Mult => used.multipliers.contains(opt),
            ConfigGate::EnemyMult => used.enemy_multipliers.contains(opt),
            // :528
            ConfigGate::Stat => used.per_stats.contains(opt) || used.enemy_multipliers.contains(opt),
            ConfigGate::TagType => used.tag_types.contains(opt),
            ConfigGate::Mod => used.mods.contains(opt),
            ConfigGate::Skill => used.skills.contains(opt),
            // :469
            ConfigGate::CondTrue => {
                let db = env.player.borrow().mod_db.clone();
                let d = db.borrow();
                d.conditions.get(opt).copied().unwrap_or(false)
            }
            // :596-599
            ConfigGate::Flag => {
                let st = st.expect("Classes/ConfigTab.lua:598 mainEnv.player.mainSkill");
                let main = st.main_skill.expect("Classes/ConfigTab.lua:598 mainEnv.player.mainSkill");
                st.skill(main).skill_flags.get(opt).copied().unwrap_or(false)
                    || env
                        .skill_mod_list(main)
                        .borrow()
                        .flag(None, &[opt])
                        .unwrap_or_else(|e| panic!("{e}"))
                        .unwrap_or(false)
            }
            // :626-636
            ConfigGate::SkillTransfigured => {
                let o = pob_mods::Value::str(opt);
                pob_mods::calc_lib::get_game_id_from_gem_name(&o, true).is_some()
                    && used
                        .skills
                        .iter()
                        .any(|s| pob_mods::calc_lib::is_gem_id_same(&pob_mods::Value::str(s), &o, true))
            }
            // :644-651
            ConfigGate::SkillFlag => st.is_some_and(|st| {
                st.active_skill_list
                    .iter()
                    .any(|s| s.skill_flags.get(opt).copied().unwrap_or(false))
            }),
            // :654-661
            ConfigGate::SkillData => st.is_some_and(|st| {
                st.active_skill_list
                    .iter()
                    .any(|s| s.skill_data.get(opt).is_some_and(pob_mods::Value::truthy))
            }),
        };
        g.memo[kind as usize].insert(opt.to_owned(), answer);
        answer
    }
}
