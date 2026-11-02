//! [`Engine`] -- the editing API of `project_15/pob-engine-wasm/src/lua/boot.lua` (`engine_*`, R8.8) over one build.
//!
//! The state is the build as the tabs' `Load` would see it: [`BuildModel`] (+ [`SaveExtras`], the tabs outside it) -- the Rust
//! run (`calcsTab:BuildOutput()`: MAIN + `EstimatePlayerProgress` + CALCS in one session, `pob_calc::calculate::calculate_model`) is derived
//! from it on every
//! [`Engine::recalc`], nothing else survives between runs (no hidden state; a new [`Engine::load_xml`] starts from scratch). A
//! mutator edits what upstream's mutator edits (the spec's allocation, `configTab.input`, a gem instance, `build.mainSocketGroup`),
//! expressed in the model, then recalculates: "mutation through the API == a fresh load of the XML with the same state" is the
//! path-equivalence gate (`tests/engine.rs`).
//!
//! Runtime state the model does not have yet -- the socket groups CalcSetup adds for item-granted skills (CalcSetup.lua:1470-1520)
//! -- is first written back through `SaveDB` + load ([`Engine::materialize`]), which is what upstream's next load would read.

use std::rc::Rc;

use pob_calc::pob_build::{self, BuildModel, InputValue};
use pob_calc::{CalculateOptions, CalculateResult, FullDpsSkill, OutTable};

use crate::display::{DisplayEnv, DisplaySettings, MinionBorrow};
use crate::save::{SaveExtras, SaveInput, save_db_xml};

/// One build loaded into the engine.
pub struct Engine {
    model: BuildModel,
    extras: SaveExtras,
    res: CalculateResult,
    /// `calcsTab.calcsEnv` (`buildOutput(CALCS)`).
    calcs: pob_calc::CalcRun,
    /// `mainOutput.SkillDPS` in the order the in-place `table.sort`s left it (Build.lua:1934, boot.lua:668): sorted once by the load's
    /// `RefreshStatList`, again by every [`Engine::display_stats_json`]; a recalc (`engine_recalc`, no `RefreshStatList`) leaves output order.
    skill_dps: Option<Vec<FullDpsSkill>>,
    settings: DisplaySettings,
}

/// The gem property `engine_setGem` edits (boot.lua:405-460).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GemProp {
    Level(f64),
    Quality(f64),
    Enabled(bool),
}

fn run(model: &BuildModel) -> Result<(CalculateResult, pob_calc::CalcRun), String> {
    pob_calc::calculate::calculate_model(Rc::new(model.clone()), CalculateOptions::default()).map_err(|e| e.0)
}

impl Engine {
    /// `engine_loadBuildXML` (boot.lua:120-130): `loadBuildFromXML` -- the tabs' `Load`, `BuildOutput`, `RefreshStatList`.
    pub fn load_xml(xml: &str) -> Result<Engine, String> {
        let model = pob_build::load_build_xml(xml).map_err(|e| format!("XML: {e}"))?;
        let extras = SaveExtras::from_xml(xml)?;
        let (res, calcs) = run(&model)?;
        let mut e = Engine {
            model,
            extras,
            res,
            calcs,
            skill_dps: None,
            settings: DisplaySettings::default(),
        };
        e.refresh_stat_list()?;
        Ok(e)
    }

    /// `Build:RefreshStatList` (Build.lua:2078) over the new output: its one `table.sort` of `SkillDPS` is the state `SaveDB` reads.
    /// Its `EstimatePlayerProgress` (Build.lua:898-913) moves `characterLevel` in auto mode -- the level of the next run and of `SaveDB`.
    fn refresh_stat_list(&mut self) -> Result<(), String> {
        let minion = MinionBorrow::new(&self.res);
        let mut denv = DisplayEnv::new(&self.res, &minion)?;
        crate::display::refresh_stat_list(&mut denv, &self.settings)?;
        self.skill_dps = denv.player.skill_dps.take();
        if self.model.character_level_auto_mode {
            self.model.level = self.res.character_level;
        }
        Ok(())
    }

    /// `engine_recalc` (boot.lua:132-150): `wipeGlobalCache()` + `calcsTab:BuildOutput()` (a fresh session over the model) -- no
    /// `RefreshStatList` (boot.lua does not run it): `SkillDPS` stays in output order, the character level stays.
    pub fn recalc(&mut self) -> Result<(), String> {
        (self.res, self.calcs) = run(&self.model)?;
        self.skill_dps = match self.res.perform.player.output.table("SkillDPS") {
            Some(OutTable::SkillDps(v)) => Some(v.clone()),
            _ => None,
        };
        Ok(())
    }

    pub fn model(&self) -> &BuildModel {
        &self.model
    }

    pub fn extras(&self) -> &SaveExtras {
        &self.extras
    }

    /// The MAIN result (`calcsTab.mainEnv` / `mainOutput`).
    pub fn result(&self) -> &CalculateResult {
        &self.res
    }

    pub fn settings(&self) -> &DisplaySettings {
        &self.settings
    }

    /// The CALCS run (`calcsTab.calcsEnv`).
    pub fn calcs_run(&self) -> &pob_calc::CalcRun {
        &self.calcs
    }

    /// `mainOutput.SkillDPS` in its current order.
    pub fn skill_dps(&self) -> Option<&[FullDpsSkill]> {
        self.skill_dps.as_deref()
    }

    /// `build:SaveDB()` (Build.lua:2277) as XML text.
    pub fn save_xml(&self) -> Result<String, String> {
        save_db_xml(&SaveInput {
            model: &self.model,
            extras: &self.extras,
            res: &self.res,
            skill_dps: self.skill_dps.as_deref(),
            calcs_env: Some(&self.calcs.env),
        })
    }

    /// The build code (ImportTab.lua:505/:632: `base64(Deflate(SaveDB("code")))`, `+/` -> `-_`).
    pub fn build_code(&self) -> Result<String, String> {
        Ok(pob_build::encode_build_code(self.save_xml()?.as_bytes()))
    }

    /// The runtime state written back into the model: `SaveDB` + the tabs' `Load` (what upstream reads on the next load).
    pub fn materialize(&mut self) -> Result<(), String> {
        let xml = self.save_xml()?;
        self.model = pob_build::load_build_xml(&xml).map_err(|e| format!("XML: {e}"))?;
        self.extras = SaveExtras::from_xml(&xml)?;
        Ok(())
    }

    // --- mutators ----------------------------------------------------------------------------------------------------------------

    /// `engine_allocNode` / `engine_deallocNode` (boot.lua:162-205): `build.spec:AllocNode(node)` / `DeallocNode(node)` on an
    /// unallocated / allocated node (no-op otherwise), `BuildAllDependsAndPaths`, recalc. Mastery nodes are refused (boot.lua:176-178).
    pub fn set_node_alloc(&mut self, id: f64, alloc: bool) -> Result<(), String> {
        let op = if alloc { "alloc" } else { "dealloc" };
        if id.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&id) {
            return Err(format!("engine_{op}Node: invalid node id '{}'", pob_lua::tostring_num(id)));
        }
        let mut spec = (*self.res.env.spec).clone();
        let i = spec
            .node_idx(id as u32)
            .ok_or_else(|| format!("engine_{op}Node: unknown node id '{}'", pob_lua::tostring_num(id)))?;
        if spec.nodes[i].ty() == pob_tree::NodeType::Mastery {
            return Err(format!(
                "engine_{op}Node: mastery nodes require effect selection and are not editable yet"
            ));
        }
        if spec.nodes[i].alloc == alloc {
            return Ok(());
        }
        if alloc {
            spec.alloc_node(i).map_err(|e| e.0)?;
        } else {
            spec.dealloc_node(i).map_err(|e| e.0)?;
        }
        let sm = active_spec_mut(&mut self.model);
        // the `nodes` SaveDB would write: sorted ids (PassiveSpec.lua:199-202, 3dbca4463 #10338)
        let mut ids = spec.alloc_table_pairs();
        ids.sort_unstable();
        sm.nodes = ids.into_iter().map(f64::from).collect();
        let mut me = pob_build::NumMap::default();
        for (m, e) in &spec.mastery_selections {
            me.set(f64::from(*m), *e);
        }
        sm.mastery_effects = me;
        self.recalc()
    }

    /// `engine_configListJSON`'s `value` / `engine_getConfig` (boot.lua:243-247, :324-330): `configTab.input[id]`, else
    /// `GetDefaultState(id, type(nil))` (the active set's placeholder, then `defaultState`).
    pub fn get_config(&self, id: &str) -> Result<Option<InputValue>, String> {
        if !crate::api::config_var_known(id) {
            return Err(format!("engine_getConfig: unknown config id '{id}'"));
        }
        let c = &self.res.env.config;
        Ok(c.input
            .get(id)
            .or_else(|| c.placeholder.get(id))
            .or_else(|| crate::save::default_state().get(id))
            .cloned())
    }

    /// `engine_setConfig` (boot.lua:336-367): `configTab.input[id] = value` (`None` = reset: `defaultState[id]`), `BuildModList`,
    /// recalc.
    pub fn set_config(&mut self, id: &str, value: Option<InputValue>) -> Result<(), String> {
        if !crate::api::config_var_known(id) {
            return Err(format!("engine_setConfig: unknown config id '{id}'"));
        }
        let v = value.or_else(|| crate::save::default_state().get(id).cloned());
        self.write_config(id, v)
    }

    /// `configTab.input[id] = nil` -- the desktop number field emptied (ConfigTab.lua:309 `input[var] = tonumber("")`), which
    /// `engine_setConfig`'s reset (`defaultState[id]`, e.g. 0) is not: a nil input falls back to the placeholder (ConfigTab.lua:1147).
    pub fn clear_config(&mut self, id: &str) -> Result<(), String> {
        if !crate::api::config_var_known(id) {
            return Err(format!("engine_setConfig: unknown config id '{id}'"));
        }
        self.write_config(id, None)
    }

    fn write_config(&mut self, id: &str, v: Option<InputValue>) -> Result<(), String> {
        let active = self.model.config.active_config_set;
        let set = self
            .model
            .config
            .sets
            .iter_mut()
            .find(|s| s.id == active)
            .ok_or("engine_setConfig: no active config set")?;
        // `input[id] = v` -- a nil write deletes the key (ConfigState::from_set replays the writes over NewConfigSet)
        set.input.insert(id.to_owned(), v);
        self.recalc()
    }

    /// `engine_setGem` (boot.lua:405-460): validates, then `gemInstance[prop] = value` (an `enabled` write on a non-Vaal gem first
    /// sets `enableGlobal1/2 = true`), `ProcessSocketGroup`, `UpdateSocketGroups`, recalc. `group`/`gem` are 1-based.
    pub fn set_gem(&mut self, group: usize, gem: usize, prop: GemProp) -> Result<(), String> {
        let groups = self.socket_group_count();
        if group < 1 || group > groups {
            return Err(format!("engine_setGem: groupIdx must be an integer in [1, {groups}], got {group}"));
        }
        let gems = self
            .res
            .env
            .skills
            .as_ref()
            .map_or(0, |s| s.tab.socket_group_list()[group - 1].gem_list.len());
        if gem < 1 || gem > gems {
            return Err(format!("engine_setGem: gemIdx must be an integer in [1, {gems}], got {gem}"));
        }
        match prop {
            GemProp::Level(v) if v.fract() != 0.0 || v < 1.0 => {
                return Err(format!(
                    "engine_setGem: level must be an integer >= 1, got {}",
                    pob_lua::tostring_num(v)
                ));
            }
            GemProp::Quality(v) if v.fract() != 0.0 || v < 0.0 => {
                return Err(format!(
                    "engine_setGem: quality must be an integer >= 0, got {}",
                    pob_lua::tostring_num(v)
                ));
            }
            _ => {}
        }
        let vaal = {
            let tab = &self.res.env.skills.as_ref().ok_or("engine_setGem: no skills")?.tab;
            let gi = &tab.socket_group_list()[group - 1].gem_list[gem - 1];
            gi.gem_data
                .as_ref()
                .and_then(|id| pob_data::gems::gems().gems.0.get(id))
                .map(|d| d.vaal_gem == Some(true))
        };
        // `build.skillsTab:UpdateSocketGroups()` (boot.lua:451) runs over the CURRENT socketGroupList -- with the groups CalcSetup added
        // for item-granted skills (CalcSetup.lua:1470-1520), which a fresh load of the original file does not have yet when its own
        // UpdateSocketGroups runs (Build.lua:688). Measured on t-cwc (LuaJIT, tools/lua-corpus/export-engine.ts): after any setGem the
        // item group "Item:22:The Hidden Blade" as main skill has GemQuality 10, after a fresh load 0. That state is the one upstream would
        // save: the model is first rebuilt from SaveDB (which holds those groups).
        if self.model_has_runtime_groups() {
            self.materialize()?;
        }
        let set = active_skill_set_mut(&mut self.model).ok_or("engine_setGem: no active skill set")?;
        let g = set
            .groups
            .get_mut(group - 1)
            .ok_or_else(|| format!("engine_setGem: socket group {group} is not in the saved state"))?;
        let gm = g
            .gems
            .get_mut(gem - 1)
            .ok_or_else(|| format!("engine_setGem: gem {gem} of group {group} is not in the saved state"))?;
        match prop {
            GemProp::Level(v) => gm.level = Some(v),
            GemProp::Quality(v) => gm.quality = Some(v),
            GemProp::Enabled(b) => {
                if vaal == Some(false) {
                    gm.enable_global1 = true;
                    gm.enable_global2 = true;
                }
                gm.enabled = b;
            }
        }
        self.recalc()
    }

    /// `(level, quality, enabled, isSupport)` of every gem of socket group `group` (1-based) -- what `engine_skillListJSON` shows.
    pub fn gem_states(&self, group: usize) -> Vec<(Option<f64>, Option<f64>, bool, bool)> {
        let Some(sk) = self.res.env.skills.as_ref() else {
            return Vec::new();
        };
        let Some(g) = group.checked_sub(1).and_then(|i| sk.tab.socket_group_list().get(i)) else {
            return Vec::new();
        };
        let data = pob_data::skills::skills();
        g.gem_list
            .iter()
            .map(|gi| {
                let effect = gi
                    .gem_data
                    .as_ref()
                    .and_then(|id| pob_data::gems::gems().gems.0.get(id))
                    .map(|d| d.granted_effect_id.clone())
                    .or_else(|| gi.granted_effect.clone());
                let support = effect
                    .and_then(|e| data.index.get(&e).map(|&x| data.effects[x].support))
                    .unwrap_or(false);
                (gi.level, gi.quality, gi.enabled, support)
            })
            .collect()
    }

    /// Whether `socketGroupList` holds groups the model does not (CalcSetup's item-granted groups).
    fn model_has_runtime_groups(&self) -> bool {
        self.socket_group_count() != pob_build::active_skill_groups(&self.model).len()
    }

    /// `engine_addItemText` (boot.lua:1336-1395): the text (`sanitiseText`) as a new item (`CreateDisplayItemFromRaw(raw, true)`:
    /// `NormaliseQuality`; `AddDisplayItem(true)`: the first free id, appended to `itemOrderList`), equipped in `slot_name` (one of
    /// ItemsTab.lua:37 `baseSlots`; Weapon 1/2 go to the Swap slot with `useSecondWeaponSet`), `PopulateSlots`, recalc. Returns the item id.
    /// The model gets the item as `ItemsTab:Save` would write it (`BuildRaw` + `ModRange`s). `CopyAnointsAndEldritchImplicits`
    /// (ItemsTab.lua:1906-1945) is not ported: when it would look at an equipped item it is an explicit error.
    pub fn add_item_text(&mut self, text: &str, slot_name: &str) -> Result<f64, String> {
        const BASE_SLOTS: [&str; 18] = [
            "Weapon 1",
            "Weapon 2",
            "Helmet",
            "Body Armour",
            "Gloves",
            "Boots",
            "Amulet",
            "Ring 1",
            "Ring 2",
            "Ring 3",
            "Belt",
            "Graft 1",
            "Graft 2",
            "Flask 1",
            "Flask 2",
            "Flask 3",
            "Flask 4",
            "Flask 5",
        ];
        if !pob_lua::find(text.as_bytes(), b"%S", None, false).ok().flatten().is_some() {
            return Err("engine_addItemText: item text must be a non-empty string".into());
        }
        if !BASE_SLOTS.contains(&slot_name) {
            return Err(format!("engine_addItemText: unknown or unsupported gear slot '{slot_name}'"));
        }
        let set = pob_build::active_item_set(&self.model);
        let effective = if matches!(slot_name, "Weapon 1" | "Weapon 2") && set.use_second_weapon_set {
            format!("{slot_name} Swap")
        } else {
            slot_name.to_owned()
        };
        let env = &self.res.env;
        let raw = pob_build::sanitise_text(text);
        let mut item = pob_items::Item::parse(
            &raw,
            &pob_items::ItemOptions {
                parse_mode: pob_calc::ParseMode::Full,
                ..Default::default()
            },
        );
        let Some(base) = item.base else {
            return Err("engine_addItemText: the item text is invalid (check rarity, name, and base type)".into());
        };
        let sel = |name: &str| {
            env.ordered_slots
                .iter()
                .find(|s| s.slot_name == name)
                .map_or(0.0, |s| s.sel_item_id)
        };
        if !pob_calc::init_env::is_item_valid_for_slot(&item, &effective, &env.spec, &env.items, sel) {
            return Err(format!(
                "engine_addItemText: item '{}' cannot be equipped in {slot_name}",
                item.name
            ));
        }
        // ItemsTab.lua:1907-1911 -- the branch that copies from the equipped item of the same type
        let item_type = if base.weapon.is_some() {
            "Weapon 1".to_owned()
        } else {
            base.ty.clone()
        };
        if let Some(s) = env.ordered_slots.iter().find(|s| s.node_id.is_none() && s.slot_name == item_type)
            && env.item(s.sel_item_id).is_some()
        {
            return Err(format!(
                "unimplemented: Classes/ItemsTab.lua:1906-1945 CopyAnointsAndEldritchImplicits from the equipped '{item_type}' item"
            ));
        }
        item.normalise_quality();
        // ItemsTab.lua:1686-1699 `AddItem`: the first id not in `self.items`
        let mut id = 1.0;
        while env.item(id).is_some() {
            id += 1.0;
        }
        let raw = item.build_raw();
        let mod_ranges = crate::save::mod_ranges(&item)
            .into_iter()
            .map(|(n, range)| pob_build::ModRangeModel { id: n as f64, range })
            .collect();
        self.model.items.push(pob_build::ItemModel {
            id,
            variant: item.variant,
            variant_alt: item.variant_alt[0],
            variant_alt2: item.variant_alt[1],
            variant_alt3: item.variant_alt[2],
            variant_alt4: item.variant_alt[3],
            variant_alt5: item.variant_alt[4],
            raw,
            mod_ranges,
        });
        self.model.item_order.push(id);
        self.set_slot_item(&effective, id);
        self.recalc()?;
        Ok(id)
    }

    /// `slot:SetSelItemId(id)` + `PopulateSlots` + recalc (ItemSlotControl.lua:72-84; the desktop's equip, and how a test undoes
    /// [`Engine::add_item_text`]): `slot_name` is a gear slot of the active item set.
    pub fn equip_item(&mut self, slot_name: &str, id: f64) -> Result<(), String> {
        if !self
            .res
            .env
            .ordered_slots
            .iter()
            .any(|s| s.node_id.is_none() && s.slot_name == slot_name)
        {
            return Err(format!("equip: unknown gear slot '{slot_name}'"));
        }
        self.set_slot_item(slot_name, id);
        self.recalc()
    }

    fn set_slot_item(&mut self, slot_name: &str, id: f64) {
        let active = pob_build::active_item_set(&self.model).id;
        let set = self
            .model
            .item_sets
            .iter_mut()
            .find(|s| s.id == active)
            .expect("the active item set");
        match set.slots.get_mut(slot_name) {
            Some(e) => e.item_id = id,
            None => {
                set.slots.insert(
                    slot_name.to_owned(),
                    pob_build::ItemSetSlotModel {
                        item_id: id,
                        active: false,
                        pb_url: None,
                    },
                );
            }
        }
    }

    /// `#build.skillsTab.socketGroupList`.
    pub fn socket_group_count(&self) -> usize {
        self.res.env.skills.as_ref().map_or(0, |s| s.tab.socket_group_list().len())
    }

    /// `engine_setMainSocketGroup` (boot.lua:462-472).
    pub fn set_main_socket_group(&mut self, group: usize) -> Result<(), String> {
        let n = self.socket_group_count();
        if group < 1 || group > n {
            return Err(format!(
                "engine_setMainSocketGroup: groupIdx must be an integer in [1, {n}], got {group}"
            ));
        }
        self.model.main_socket_group = group as f64;
        self.recalc()
    }

    /// `engine_statsForJSON` (boot.lua:482-500): the scalar output as if `group` were the main socket group; the build is restored.
    pub fn stats_for(&mut self, group: usize) -> Result<indexmap::IndexMap<String, pob_calc::StatValue>, String> {
        let n = self.socket_group_count();
        if group < 1 || group > n {
            return Err(format!("engine_statsForJSON: groupIdx must be an integer in [1, {n}], got {group}"));
        }
        let original = self.model.main_socket_group;
        self.model.main_socket_group = group as f64;
        let r = self.recalc().map(|()| self.res.stats.clone());
        self.model.main_socket_group = original;
        self.recalc()?;
        r
    }

    /// [`Engine::stats_for`] as `engine_outputJSON` shapes it (without `_meta`).
    pub fn stats_for_json(&mut self, group: usize) -> Result<serde_json::Value, String> {
        Ok(crate::api::stats_json(&self.stats_for(group)?))
    }

    // --- readers -------------------------------------------------------------------------------------------------------------------

    /// `engine_outputJSON` (boot.lua:790-...): every scalar of `mainOutput` (numbers; NaN/inf as strings), in output order.
    pub fn output_json(&self) -> serde_json::Value {
        crate::api::output_json(&self.res)
    }

    /// `engine_fullDPSJSON` (boot.lua:506-520): `mainOutput.SkillDPS` as it is.
    pub fn full_dps_json(&self) -> serde_json::Value {
        crate::api::full_dps_json(self.skill_dps.as_deref().unwrap_or(&[]))
    }

    /// `engine_displayStatsJSON` (boot.lua:776-788) -- sorts `SkillDPS` in place again (boot.lua:668).
    pub fn display_stats_json(&mut self) -> Result<serde_json::Value, String> {
        let minion = MinionBorrow::new(&self.res);
        let mut denv = DisplayEnv::new(&self.res, &minion)?;
        denv.player.skill_dps = self.skill_dps.take();
        let sections = crate::display::display_sections(&mut denv, &self.settings);
        self.skill_dps = denv.player.skill_dps.take();
        Ok(crate::api::display_json(&sections?))
    }

    /// `engine_skillListJSON` (boot.lua:369-403).
    pub fn skill_list_json(&self) -> serde_json::Value {
        crate::api::skill_list_json(&self.res)
    }

    /// `engine_powerStart` + `engine_powerStep` until done + `engine_powerReportJSON` (boot.lua:1779-1990) as one call: `stat_id` = a
    /// `treeTab.powerStatList` stat (`None` = its first entry, Offence/Defence); the walk runs on `threads` threads (each with its own session
    /// in the state `BuildOutput` leaves, `pob_calc::power::build_power_threads` -- the one-thread result) and `cancel` abandons it
    /// (`Ok(None)`, as `engine_powerCancel`). The wasm API's stepping exists for a single-threaded event loop; a step-wise walk over one session
    /// is `pob_calc::power::PowerBuilder`.
    pub fn power_report_json(
        &self,
        stat_id: Option<&str>,
        threads: usize,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Option<serde_json::Value>, String> {
        use pob_calc::power::{PowerStat, ReportDist, build_power_report_list, build_power_threads, power_stat_list};
        // TreeTab.lua:264-269 `powerStatList`: data.powerStatList without the `ignoreForNodes` entries
        let list: Vec<PowerStat> = power_stat_list().into_iter().filter(|p| !p.ignore_for_nodes).collect();
        let stat = match stat_id {
            None => list.first().cloned(),
            Some(id) => Some(
                list.iter()
                    .find(|p| p.stat.as_deref() == Some(id))
                    .cloned()
                    .ok_or_else(|| format!("engine_powerStart: unknown power stat id '{id}'"))?,
            ),
        };
        let Some(result) = build_power_threads(&self.model, CalculateOptions::default(), stat.as_ref(), None, threads, cancel)? else {
            return Ok(None);
        };
        // TreeTab.lua:1072-1086: the displayStats entry of the stat formats the numbers
        let ds = stat
            .as_ref()
            .and_then(|p| p.stat.as_deref())
            .and_then(|st| crate::stats::DISPLAY_STATS.iter().find(|d| d.stat == Some(st)));
        let fmt = match ds {
            None => None,
            Some(d) => Some(pob_calc::power::DisplayStatFmt {
                fmt: d
                    .fmt
                    .ok_or("Classes/TreeTab.lua:1092: attempt to concatenate field 'fmt' (a nil value)")?
                    .to_owned(),
                pc: d.pc,
                mod_: d.mod_,
                lower_is_better: d.lower_is_better,
            }),
        };
        let env = &self.res.env;
        let report = build_power_report_list(
            &env.spec,
            &env.granted_passives,
            &result,
            stat.as_ref(),
            fmt.as_ref(),
            &self.settings.number_format(),
        );
        // PowerReportListControl:ReList (:102-123) with the control's defaults (unallocated, no clusters, masteries shown)
        let entries: Vec<serde_json::Value> = report
            .iter()
            .filter(|e| e.power > 0.0 && e.path_dist != ReportDist::Str("Cluster") && !e.allocated)
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "type": e.ty.as_str(),
                    "power": e.power,
                    "powerStr": crate::display::strip_escapes(&e.power_str),
                    "pathPower": e.path_power,
                    "pathPowerStr": crate::display::strip_escapes(&e.path_power_str),
                    "pathDist": match &e.path_dist { ReportDist::Num(n) => serde_json::json!(n), ReportDist::Str(s) => serde_json::json!(s) },
                    "allocated": e.allocated,
                })
            })
            .collect();
        Ok(Some(serde_json::json!({
            "stat": stat.as_ref().and_then(|p| p.stat.clone()),
            "label": stat.as_ref().map(|p| p.label.clone()),
            "count": entries.len(),
            "entries": entries,
        })))
    }

    /// `engine_configListJSON` (boot.lua:249-322) without `eligible` (the ConfigTab control's `shown` closure -- not ported).
    pub fn config_list_json(&self) -> serde_json::Value {
        crate::api::config_list_json(&self.res.env.config)
    }
}

/// The `<Spec>` [`pob_build::active_spec`] picks, mutable.
fn active_spec_mut(m: &mut BuildModel) -> &mut pob_build::SpecModel {
    let target: *const pob_build::SpecModel = pob_build::active_spec(m);
    m.tree
        .specs
        .iter_mut()
        .find(|s| std::ptr::eq(&**s, target))
        .expect("the active spec is one of the specs")
}

/// The `<SkillSet>` [`pob_build::active_skill_groups`] picks, mutable.
fn active_skill_set_mut(m: &mut BuildModel) -> Option<&mut pob_build::SkillSetModel> {
    let id = m.skills.active_skill_set;
    let i = m
        .skills
        .sets
        .iter()
        .position(|s| s.id == id)
        .or(if m.skills.sets.is_empty() { None } else { Some(0) })?;
    m.skills.sets.get_mut(i)
}
