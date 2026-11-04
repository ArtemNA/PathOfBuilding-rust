//! What `Classes/SkillsTab.lua`, `SkillListControl.lua`, `SkillSetListControl.lua` and `GemSelectControl.lua` ask of the build (U5).
//!
//! Upstream's tab mutates live `socketGroup` / `gemInstance` tables; the next `BuildOutput` reads them. Here a run starts from the
//! model, so every mutator edits [`Engine::skills_tab`] (the live copy of `build.skillsTab`: what the last run left, with what CalcSetup
//! wrote -- `displayLabel`, `slotEnabled`, the groups of item-granted skills -- until the next run replaces them, as upstream's labels stay stale
//! until the `buildFlag` frame) and writes the result to the model exactly as `SkillsTab:Save` + `Load` would. Like the controls' callbacks they do
//! not recalculate: the caller sets `buildFlag`, the next frame runs [`Engine::refresh`]. Path equivalence of each: `tests/skills_tab.rs`.

use std::rc::Rc;

use pob_calc::Env;
use pob_calc::calc_tools::{ActiveSkillView, WeaponView, can_granted_effect_support_active_skill_ext};
use pob_calc::env::{CalcOverride, GemOverride};
use pob_calc::pob_build::{self, GemModel, InputValue, SkillSetModel, SocketGroupModel};
use pob_calc::session::RunRef;
use pob_calc::skills_tab::{GemInstance, SkillSet, SkillsTab, SocketGroup, process_gem_level, process_socket_group};
use pob_data::skills::Skills;

use crate::engine::{Engine, active_skill_set_mut};

/// `CreateUndoState` (SkillsTab.lua:1397-1419).
#[derive(Clone)]
pub struct SkillsUndoState {
    pub skill_sets: Vec<SkillSet>,
    pub active_skill_set_id: f64,
    /// `build.mainSocketGroup`.
    pub active_socket_group: f64,
    /// `build.calcsTab.input.skill_number`.
    pub active_socket_group2: Option<InputValue>,
}

/// What `calcFunc` runs with: the temporary edit of a gem instance in the tab's tooltips.
#[derive(Clone, Debug, PartialEq)]
pub enum GemEdit {
    /// `CalcOutputWithThisGem` (GemSelectControl.lua:70-114).
    Candidate { gem_id: String, imbued: bool },
    /// SkillsTab.lua:940-943
    Quality20,
    /// SkillsTab.lua:980-982
    ToggleEnabled,
}

/// One slot of the tab's `dpsField`: `(dpsField == "FullDPS" and o[f]) or (o.Minion and o.Minion.CombinedDPS) or o[f] or 0` (GemSelectControl.lua:354, :430).
fn dps_of(run: &RunRef, field: &str) -> f64 {
    let r = run.borrow();
    let own = r.state.player.output.opt_num(field);
    if field == "FullDPS"
        && let Some(v) = own
    {
        return v;
    }
    if let Some(m) = &r.state.minion
        && let Some(v) = m.borrow().output.opt_num("CombinedDPS")
    {
        return v;
    }
    own.unwrap_or(0.0)
}

fn gem_to_model(gi: &GemInstance) -> GemModel {
    let gd = gi.gem_data.as_ref().and_then(|id| pob_data::gems::gems().gems.0.get(id));
    GemModel {
        name_spec: gi.name_spec.clone().unwrap_or_default(),
        gem_id: gd.map(|d| d.game_id.clone()),
        variant_id: gd.map(|d| d.variant_id.clone()),
        skill_id: gi.skill_id.clone(),
        level: gi.level,
        quality: gi.quality,
        quality_id: None,
        enabled: gi.enabled,
        // `tostring(nil)` reads back as `"nil"`: not true (SkillsTab.lua:499-500)
        enable_global1: gi.enable_global1.unwrap_or(false),
        enable_global2: gi.enable_global2.unwrap_or(false),
        count: gi.count.unwrap_or(1.0),
        skill_part: gi.skill_part,
        skill_part_calcs: gi.skill_part_calcs,
        skill_stage_count: gi.skill_stage_count,
        skill_stage_count_calcs: gi.skill_stage_count_calcs,
        skill_mine_count: gi.skill_mine_count,
        skill_mine_count_calcs: gi.skill_mine_count_calcs,
        skill_minion: gi.skill_minion.clone(),
        skill_minion_calcs: gi.skill_minion_calcs.clone(),
        skill_minion_item_set: gi.skill_minion_item_set,
        skill_minion_item_set_calcs: gi.skill_minion_item_set_calcs,
        skill_minion_skill: gi.skill_minion_skill,
        skill_minion_skill_calcs: gi.skill_minion_skill_calcs,
    }
}

fn group_to_model(g: &SocketGroup) -> SocketGroupModel {
    SocketGroupModel {
        enabled: g.enabled,
        include_in_full_dps: g.include_in_full_dps == Some(true),
        include_in_full_dps_attr: g.include_in_full_dps.is_some(),
        group_count: g.group_count,
        label: g.label.clone(),
        slot: g.slot.clone(),
        source: g.source.clone(),
        main_active_skill: g.main_active_skill.unwrap_or(1.0),
        main_active_skill_calcs: g.main_active_skill_calcs.unwrap_or(1.0),
        imbued_support: g.imbued_support.clone().filter(|_| g.slot.is_some()),
        gems: g.gem_list.iter().map(gem_to_model).collect(),
    }
}

/// `{ label = "", enabled = true, gemList = { } }` (SkillListControl.lua:60-64).
fn new_group() -> SocketGroup {
    SocketGroup {
        label: Some(String::new()),
        enabled: true,
        ..SocketGroup::new_empty()
    }
}

/// `{ nameSpec = "", level, quality, enabled = true, enableGlobal1 = true, enableGlobal2 = true, count = 1, new = true }`.
fn new_gem(level: Option<f64>, quality: f64) -> GemInstance {
    GemInstance {
        name_spec: Some(String::new()),
        level,
        quality: Some(quality),
        enabled: true,
        enable_global1: Some(true),
        enable_global2: Some(true),
        count: Some(1.0),
        new: Some(true),
        ..GemInstance::default()
    }
}

/// `mainSocketGroup` / `skill_number` after a list move (SkillListControl.lua:130-149).
fn moved_index(cur: f64, from: f64, to: f64) -> f64 {
    if cur == from {
        to
    } else if cur > from && cur <= to {
        cur - 1.0
    } else if cur < from && cur >= to {
        cur + 1.0
    } else {
        cur
    }
}

/// SkillsTab.lua:1464-1475 over the live list.
pub(crate) fn rebuild_imbued(tab: &mut SkillsTab) {
    let data = pob_data::gems::gems();
    let mut out = indexmap::IndexMap::new();
    for sg in tab.socket_group_list() {
        if let (Some(slot), Some(imbued)) = (&sg.slot, &sg.imbued_support) {
            let key = format!("{} support", imbued.to_ascii_lowercase());
            if let Some(gem) = data.gem_for_base_name.get(&key).and_then(|id| data.gems.get(id)) {
                out.insert(slot.clone(), gem.granted_effect_id.clone());
            }
        }
    }
    tab.imbued_support_by_slot = out;
}

fn lua_err(e: pob_calc::calc_tools::LuaError) -> String {
    e
}

impl Engine {
    pub(crate) fn skill_data(&self) -> Rc<Skills> {
        self.result().env.skills.as_ref().expect("skills on").skills.clone()
    }

    /// `build.skillsTab` as it is now.
    pub fn skills_tab(&self) -> &SkillsTab {
        match self.skills_live_ref() {
            Some(t) => t,
            None => &self.result().env.skills.as_ref().expect("skills on").tab,
        }
    }

    /// `self.socketGroupList`.
    pub fn socket_groups(&self) -> &[SocketGroup] {
        self.skills_tab().socket_group_list()
    }

    pub(crate) fn edit_skills<R>(&mut self, f: impl FnOnce(&mut SkillsTab, &Skills) -> Result<R, String>) -> Result<R, String> {
        let skills = self.skill_data();
        let mut tab = match self.take_skills_live() {
            Some(t) => t,
            None => self.merged_skills_tab()?,
        };
        let r = f(&mut tab, &skills);
        self.write_skills_to_model(&tab);
        self.put_skills_live(tab);
        r
    }

    /// What `SaveDB` + `Load` carries of the tab into the model.
    fn write_skills_to_model(&mut self, tab: &SkillsTab) {
        self.nil_main = nil_main_of(tab);
        let m = &mut self.model_mut().skills;
        m.default_gem_level.clone_from(&tab.default_gem_level);
        m.default_gem_quality = tab.default_gem_quality;
        m.active_skill_set = tab.active_skill_set_id;
        m.sets = tab
            .skill_sets
            .iter()
            .map(|s| SkillSetModel {
                id: s.id,
                title: s.title.clone(),
                groups: s.socket_group_list.iter().map(group_to_model).collect(),
            })
            .collect();
    }

    // --- socket groups ---------------------------------------------------------------------------------------------------------------

    /// The `New` button (SkillListControl.lua:59-72): the new group's index.
    pub fn new_socket_group(&mut self) -> Result<usize, String> {
        self.edit_skills(|t, _| {
            let l = t.socket_group_list_mut();
            l.push(new_group());
            Ok(l.len())
        })
    }

    /// `t_remove(self.list, index)` + `RebuildImbuedSupportBySlot` + `updateActiveSocketGroupIndex` (SkillListControl.lua:162-171, :175-180).
    pub fn delete_socket_group(&mut self, index: usize) -> Result<(), String> {
        self.edit_skills(|t, _| {
            if index == 0 || index > t.socket_group_list().len() {
                return Err(format!("SkillListControl: no socket group {index}"));
            }
            t.socket_group_list_mut().remove(index - 1);
            rebuild_imbued(t);
            Ok(())
        })?;
        let i = index as f64;
        let main = self.main_socket_group();
        if main > i {
            self.set_main_pending(main - 1.0);
        }
        if let Some(n) = self.calcs_skill_number()
            && n > i
        {
            self.set_calcs_skill_number(n - 1.0);
        }
        Ok(())
    }

    /// The `Delete All` confirm (SkillListControl.lua:46-54): `wipeTable(self.list)`.
    pub fn delete_all_socket_groups(&mut self) -> Result<(), String> {
        self.edit_skills(|t, _| {
            t.socket_group_list_mut().clear();
            rebuild_imbued(t);
            Ok(())
        })
    }

    /// The drag of a row (ListControl.lua:449-456) + `OnOrderChange(selIndex, selDragIndex)` (SkillListControl.lua:130-149).
    pub fn move_socket_group(&mut self, from: usize, to: usize) -> Result<(), String> {
        self.edit_skills(|t, _| {
            let l = t.socket_group_list_mut();
            let g = l.remove(from - 1);
            l.insert(to - 1, g);
            Ok(())
        })?;
        let (f, n) = (from as f64, to as f64);
        let main = self.main_socket_group();
        self.set_main_pending(moved_index(main, f, n));
        if let Some(c) = self.calcs_skill_number() {
            self.set_calcs_skill_number(moved_index(c, f, n));
        }
        Ok(())
    }

    /// The right click on a row (SkillListControl.lua:218-223).
    pub fn set_main_group(&mut self, index: usize) {
        self.set_main_pending(index as f64);
    }

    fn with_group<R>(&mut self, index: usize, f: impl FnOnce(&mut SocketGroup, &Skills) -> Result<R, String>) -> Result<R, String> {
        self.edit_skills(|t, s| {
            let g = t
                .socket_group_list_mut()
                .get_mut(index.wrapping_sub(1))
                .ok_or_else(|| format!("SkillsTab: no socket group {index}"))?;
            f(g, s)
        })
    }

    /// `controls.groupLabel` (SkillsTab.lua:165-170).
    pub fn set_group_label(&mut self, index: usize, label: &str) -> Result<(), String> {
        self.with_group(index, |g, s| {
            g.label = Some(label.to_owned());
            process_socket_group(g, s).map_err(lua_err)
        })
    }

    /// `controls.groupSlot` (SkillsTab.lua:172-188). `Ok(true)`: the imbued control was emptied (`imbuedSupport.gemId = nil`, `SetText("")`).
    pub fn set_group_slot(&mut self, index: usize, slot: Option<&str>) -> Result<bool, String> {
        self.edit_skills(|t, _| {
            let (old_slot, imbued) = {
                let g = &t.socket_group_list()[index - 1];
                (g.slot.clone(), g.imbued_support.is_some())
            };
            let mut cleared = false;
            if let Some(old) = old_slot.as_ref().filter(|o| imbued && t.imbued_support_by_slot.contains_key(*o)) {
                let cur = t.imbued_support_by_slot.get(old).cloned();
                match slot {
                    Some(new) if !t.imbued_support_by_slot.contains_key(new) => {
                        t.imbued_support_by_slot.insert(new.to_owned(), cur.expect("checked"));
                    }
                    _ => {
                        cleared = true;
                        t.socket_group_list_mut()[index - 1].imbued_support = None;
                    }
                }
                t.imbued_support_by_slot.shift_remove(old);
            }
            t.socket_group_list_mut()[index - 1].slot = slot.map(str::to_owned);
            Ok(cleared)
        })
    }

    /// `controls.groupEnabled` (SkillsTab.lua:207-211), Ctrl+click on a row (SkillListControl.lua:226).
    pub fn set_group_enabled(&mut self, index: usize, state: bool) -> Result<(), String> {
        self.with_group(index, |g, _| {
            g.enabled = state;
            Ok(())
        })
    }

    /// `controls.includeInFullDPS` (:212-216), Ctrl+right click on a row (SkillListControl.lua:211).
    pub fn set_group_full_dps(&mut self, index: usize, state: bool) -> Result<(), String> {
        self.with_group(index, |g, _| {
            g.include_in_full_dps = Some(state);
            Ok(())
        })
    }

    /// `controls.groupCount` (:375-379).
    pub fn set_group_count(&mut self, index: usize, count: Option<f64>) -> Result<(), String> {
        self.with_group(index, |g, _| {
            g.group_count = Some(count.unwrap_or(1.0));
            Ok(())
        })
    }

    /// `ProcessSocketGroup` of a shown group (`SetDisplayGroup`, :1278).
    pub fn process_group(&mut self, index: usize) -> Result<(), String> {
        self.with_group(index, |g, s| process_socket_group(g, s).map_err(lua_err))
    }

    /// `PasteSocketGroup` (SkillsTab.lua:708-740): the new group's index; `None`: no gem in the text.
    pub fn paste_socket_group(&mut self, text: &str) -> Result<Option<usize>, String> {
        let skill_text = pob_build::sanitise_text(text);
        let mut g = new_group();
        let capture = |pat: &[u8]| -> Option<String> {
            let c = pob_lua::match_(skill_text.as_bytes(), pat, None).ok().flatten()?;
            c.get(0)
                .and_then(|c| c.bytes(skill_text.as_bytes()))
                .map(|b| String::from_utf8_lossy(b).into_owned())
        };
        if let Some(l) = capture(b"Label: (%C+)") {
            g.label = Some(l);
        }
        if let Some(s) = capture(b"Slot: (%C+)") {
            g.slot = Some(s);
        }
        let bytes = skill_text.as_bytes();
        for c in pob_lua::gmatch(bytes, b"([ %a']+) (%d+)/(%d+) ?(%a*) (%d+)") {
            let c = c.map_err(|e| e.0)?;
            let cap = |i: usize| String::from_utf8_lossy(c.get(i).and_then(|c| c.bytes(bytes)).unwrap_or(b"")).into_owned();
            g.gem_list.push(GemInstance {
                name_spec: Some(cap(0)),
                level: Some(cap(1).parse().unwrap_or(20.0)),
                quality: Some(cap(2).parse().unwrap_or(0.0)),
                enabled: cap(3) != "DISABLED",
                count: Some(cap(4).parse().unwrap_or(1.0)),
                enable_global1: Some(true),
                enable_global2: Some(true),
                ..GemInstance::default()
            });
        }
        if g.gem_list.is_empty() {
            return Ok(None);
        }
        // `SetDisplayGroup(newGroup)`: ProcessSocketGroup
        self.edit_skills(|t, s| {
            process_socket_group(&mut g, s).map_err(lua_err)?;
            let l = t.socket_group_list_mut();
            l.push(g);
            Ok(Some(l.len()))
        })
    }

    /// `CopySocketGroup` (SkillsTab.lua:694-706): the clipboard text.
    pub fn copy_socket_group_text(&self, index: usize) -> Result<String, String> {
        use pob_lua::format::{Arg, format};
        let g = self
            .socket_groups()
            .get(index.wrapping_sub(1))
            .ok_or_else(|| format!("SkillsTab: no socket group {index}"))?;
        let mut t = String::new();
        if let Some(l) = g.label.as_ref().filter(|l| l.bytes().any(|c| !c.is_ascii_whitespace())) {
            t.push_str(&format!("Label: {l}\r\n"));
        }
        if let Some(s) = &g.slot {
            t.push_str(&format!("Slot: {s}\r\n"));
        }
        for gi in &g.gem_list {
            let name = gi.name_spec.clone().unwrap_or_default();
            t.push_str(&format(
                "%s %d/%d %s %d\r\n",
                &[
                    Arg::Str(&name),
                    Arg::Num(gi.level.ok_or("Classes/SkillsTab.lua:703: bad argument #3 to 'format'")?),
                    Arg::Num(gi.quality.ok_or("Classes/SkillsTab.lua:703: bad argument #4 to 'format'")?),
                    Arg::Str(if gi.enabled { "" } else { "DISABLED" }),
                    Arg::Num(gi.count.unwrap_or(1.0)),
                ],
            )?);
        }
        Ok(t)
    }

    // --- gems ------------------------------------------------------------------------------------------------------------------------

    fn with_gem<R>(
        &mut self,
        group: usize,
        gem: usize,
        f: impl FnOnce(&mut SocketGroup, usize, &Skills) -> Result<R, String>,
    ) -> Result<R, String> {
        self.with_group(group, |g, s| {
            if gem == 0 || gem > g.gem_list.len() {
                return Err(format!("SkillsTab: socket group {group} has no gem {gem}"));
            }
            f(g, gem - 1, s)
        })
    }

    /// `deleteGem` (SkillsTab.lua:747-763) without its slot controls; `UpdateSocketGroups` is the next run's.
    pub fn delete_gem(&mut self, group: usize, gem: usize) -> Result<(), String> {
        self.with_gem(group, gem, |g, i, _| {
            g.gem_list.remove(i);
            Ok(())
        })
    }

    /// The gem-less part of the name callback (SkillsTab.lua:795-805): `gemList[index]` of the empty slot.
    pub fn add_empty_gem(&mut self, group: usize) -> Result<(), String> {
        let quality = self.skills_tab().default_gem_quality;
        self.with_group(group, |g, _| {
            g.gem_list.push(new_gem(Some(1.0), quality));
            Ok(())
        })
    }

    /// The level / quality / enabled / count callbacks' first branch (:845-846, :867-868, :958-959, :997-998): `level = self.defaultGemLevel or 20`
    /// is a STRING upstream (`normalMaximum`) until `ProcessSocketGroup` validates it.
    pub fn add_gem_from_field(&mut self, group: usize) -> Result<(), String> {
        let quality = self.skills_tab().default_gem_quality;
        self.with_group(group, |g, _| {
            g.gem_list.push(new_gem(None, quality));
            Ok(())
        })
    }

    /// The name callback (SkillsTab.lua:823-830): `gemInstance.gemId = gemId`, `skillId = nil`, `ProcessSocketGroup`, `level = ProcessGemLevel(gemData)`.
    /// Returns `(level, count)` for the slot's fields.
    pub fn set_gem_id(&mut self, group: usize, gem: usize, gem_id: Option<&str>) -> Result<(f64, f64), String> {
        let default_level = self.skills_tab().default_gem_level.clone();
        let character_level = self.model().level;
        self.with_gem(group, gem, |g, i, s| {
            {
                let gi = &mut g.gem_list[i];
                gi.gem_id = gem_id.map(str::to_owned);
                gi.skill_id = None;
            }
            process_socket_group(g, s).map_err(lua_err)?;
            let gi = &mut g.gem_list[i];
            let gd = gi
                .gem_data
                .as_ref()
                .and_then(|id| pob_data::gems::gems().gems.0.get(id))
                .ok_or("Classes/SkillsTab.lua:1130: attempt to index local 'gemData' (a nil value)")?;
            let level = process_gem_level(gd, false, &default_level, Some(character_level), s).map_err(lua_err)?;
            gi.level = Some(level);
            gi.natural_max_level = Some(level);
            Ok((level, gi.count.unwrap_or(1.0)))
        })
    }

    /// `gemInstance.gemId` of the slot (:814).
    pub fn gem_id_at(&self, group: usize, gem: usize) -> Option<String> {
        self.socket_groups()
            .get(group.wrapping_sub(1))?
            .gem_list
            .get(gem.wrapping_sub(1))?
            .gem_id
            .clone()
    }

    /// `slot.level` (:852-853): `tonumber(buf) or naturalMaxLevel or ProcessGemLevel(gemData) or 20`.
    pub fn set_gem_level(&mut self, group: usize, gem: usize, buf: Option<f64>) -> Result<(), String> {
        let default_level = self.skills_tab().default_gem_level.clone();
        let character_level = self.model().level;
        self.with_gem(group, gem, |g, i, s| {
            let gi = &mut g.gem_list[i];
            let level = match (buf, gi.natural_max_level) {
                (Some(v), _) => v,
                (None, Some(n)) => n,
                (None, None) => {
                    let gd = gi
                        .gem_data
                        .as_ref()
                        .and_then(|id| pob_data::gems::gems().gems.0.get(id))
                        .ok_or("Classes/SkillsTab.lua:1130: attempt to index local 'gemData' (a nil value)")?;
                    process_gem_level(gd, false, &default_level, Some(character_level), s).map_err(lua_err)?
                }
            };
            gi.level = Some(level);
            process_socket_group(g, s).map_err(lua_err)
        })
    }

    /// `slot.quality` (:874-875): `tonumber(buf) or self.defaultGemQuality or 0`.
    pub fn set_gem_quality(&mut self, group: usize, gem: usize, buf: Option<f64>) -> Result<(), String> {
        let default_quality = self.skills_tab().default_gem_quality;
        self.with_gem(group, gem, |g, i, s| {
            g.gem_list[i].quality = Some(buf.unwrap_or(default_quality));
            process_socket_group(g, s).map_err(lua_err)
        })
    }

    /// `slot.enabled` (:964-971). `true` = not a Vaal gem: `enableGlobal1/2` were set.
    pub fn set_gem_enabled(&mut self, group: usize, gem: usize, state: bool) -> Result<bool, String> {
        self.with_gem(group, gem, |g, i, s| {
            let gi = &mut g.gem_list[i];
            let vaal = gi
                .gem_data
                .as_ref()
                .and_then(|id| pob_data::gems::gems().gems.0.get(id))
                .ok_or("Classes/SkillsTab.lua:964: attempt to index field 'gemData' (a nil value)")?
                .vaal_gem
                == Some(true);
            if !vaal {
                gi.enable_global1 = Some(true);
                gi.enable_global2 = Some(true);
            }
            gi.enabled = state;
            process_socket_group(g, s).map_err(lua_err)?;
            Ok(!vaal)
        })
    }

    /// `slot.count` (:1004-1006): the value shown back.
    pub fn set_gem_count(&mut self, group: usize, gem: usize, buf: Option<f64>) -> Result<f64, String> {
        self.with_gem(group, gem, |g, i, s| {
            let v = buf.unwrap_or(1.0);
            g.gem_list[i].count = Some(v);
            process_socket_group(g, s).map_err(lua_err)?;
            Ok(v)
        })
    }

    /// `slot.enableGlobal1/2` (:1042-1047, :1061-1066).
    pub fn set_gem_enable_global(&mut self, group: usize, gem: usize, which: u8, state: bool) -> Result<(), String> {
        self.with_gem(group, gem, |g, i, _| {
            let gi = &mut g.gem_list[i];
            if which == 1 {
                gi.enable_global1 = Some(state);
            } else {
                gi.enable_global2 = Some(state);
            }
            Ok(())
        })
    }

    /// `controls.imbuedSupport`'s callback (SkillsTab.lua:322-350). `display`: the shown group, when `targetSlot == displayGroup.slot`. `gem_id`: `None` when the
    /// buffer matches no gem. Returns `(name, buildFlag)`.
    pub fn set_imbued_support(
        &mut self,
        slot: &str,
        display: Option<usize>,
        gem_id: Option<&str>,
    ) -> Result<(Option<String>, bool), String> {
        self.edit_skills(|t, _| match gem_id {
            Some(id) => {
                let gem = pob_data::gems::gems()
                    .gems
                    .0
                    .get(id)
                    .ok_or_else(|| format!("Classes/SkillsTab.lua:333: no data.gems['{id}']"))?;
                t.imbued_support_by_slot.insert(slot.to_owned(), gem.granted_effect_id.clone());
                if let Some(d) = display {
                    t.socket_group_list_mut()[d - 1].imbued_support = Some(gem.name.clone());
                }
                Ok((Some(gem.name.clone()), true))
            }
            None => {
                let clean = t.imbued_support_by_slot.shift_remove(slot);
                if let Some(d) = display {
                    t.socket_group_list_mut()[d - 1].imbued_support = None;
                }
                Ok((None, clean.is_some()))
            }
        })
    }

    /// `build.calcsTab.input.skill_number`.
    fn calcs_skill_number(&self) -> Option<f64> {
        match self.calcs_input().get("skill_number") {
            Some(InputValue::Num(n)) => Some(*n),
            _ => None,
        }
    }

    fn set_calcs_skill_number(&mut self, n: f64) {
        self.set_calcs_input("skill_number", Some(InputValue::Num(n)));
    }

    fn clear_calcs_skill_number(&mut self) {
        self.set_calcs_input("skill_number", None);
    }

    // --- the options of the tab ------------------------------------------------------------------------------------------------------

    /// `controls.defaultLevel`.
    pub fn set_default_gem_level(&mut self, level: &str) -> Result<(), String> {
        self.edit_skills(|t, _| {
            t.default_gem_level = level.to_owned();
            Ok(())
        })
    }

    /// `controls.defaultQuality`: `m_min(tonumber(buf) or 0, 23)`.
    pub fn set_default_gem_quality(&mut self, buf: Option<f64>) -> Result<(), String> {
        self.extras_mut().nil_default_gem_quality = false;
        self.edit_skills(|t, _| {
            t.default_gem_quality = pob_lua::number::min2(buf.unwrap_or(0.0), 23.0);
            Ok(())
        })
    }

    pub fn set_sort_gems_by_dps(&mut self, state: bool) {
        self.model_mut().skills.sort_gems_by_dps = Some(state);
    }

    pub fn set_sort_gems_by_dps_field(&mut self, field: &str) {
        self.model_mut().skills.sort_gems_by_dps_field = field.to_owned();
    }

    pub fn set_show_support_gem_types(&mut self, show: &str) {
        self.model_mut().skills.show_support_gem_types = show.to_owned();
    }

    pub fn set_show_legacy_gems(&mut self, state: bool) {
        self.model_mut().skills.show_legacy_gems = Some(state);
    }

    // --- skill sets ------------------------------------------------------------------------------------------------------------------

    /// `SetActiveSkillSet(skillSetId)` (SkillsTab.lua:1478-1502) without its controls: the id it picked.
    pub fn set_active_skill_set(&mut self, id: f64) -> Result<f64, String> {
        self.edit_skills(|t, _| {
            let id = if t.skill_sets.iter().any(|s| s.id == id) {
                id
            } else {
                t.skill_sets.first().ok_or("SkillsTab: no skill set")?.id
            };
            t.active_skill_set_id = id;
            rebuild_imbued(t);
            Ok(id)
        })
    }

    /// `(id, title)` in `skillSetOrderList` order.
    pub fn skill_sets(&self) -> Vec<(f64, Option<String>)> {
        self.skills_tab().skill_sets.iter().map(|s| (s.id, s.title.clone())).collect()
    }

    /// `NewSkillSet()` + the `Save` of `RenameSet(.., true)`: the first free id, at the end of the order (SkillsTab.lua:1452-1462, SkillSetListControl.lua:67-73).
    pub fn new_skill_set(&mut self, title: &str) -> Result<f64, String> {
        self.edit_skills(|t, _| {
            let id = first_free_set(t);
            t.skill_sets.push(SkillSet {
                id,
                title: Some(title.to_owned()),
                socket_group_list: Vec::new(),
            });
            Ok(id)
        })
    }

    /// The `Copy` button + the popup's `Save` (SkillSetListControl.lua:20-38).
    pub fn copy_skill_set(&mut self, source: f64, title: &str) -> Result<f64, String> {
        self.edit_skills(|t, _| {
            let mut copy = t
                .skill_sets
                .iter()
                .find(|s| s.id == source)
                .cloned()
                .ok_or_else(|| format!("SkillSetListControl: no skill set {source}"))?;
            copy.id = first_free_set(t);
            copy.title = Some(title.to_owned());
            let id = copy.id;
            t.skill_sets.push(copy);
            Ok(id)
        })
    }

    pub fn rename_skill_set(&mut self, id: f64, title: &str) -> Result<(), String> {
        self.edit_skills(|t, _| {
            t.skill_sets
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| format!("SkillSetListControl: no skill set {id}"))?
                .title = Some(title.to_owned());
            Ok(())
        })
    }

    /// `t_remove(self.list, index)`, `skillSets[id] = nil` (SkillSetListControl.lua:110-111); the caller changes the active set when it was this one.
    pub fn delete_skill_set(&mut self, id: f64) -> Result<(), String> {
        let active = self.skills_tab().active_skill_set_id;
        self.edit_skills(|t, _| {
            let i = t
                .skill_sets
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| format!("SkillSetListControl: no skill set {id}"))?;
            t.skill_sets.remove(i);
            // the list stays readable until `SetActiveSkillSet`
            if id == active {
                t.active_skill_set_id = t.skill_sets.get(i.saturating_sub(1)).ok_or("SkillSetListControl: last set")?.id;
            }
            Ok(())
        })
    }

    /// `skillSetOrderList` after a drag (`OnOrderChange`: `modFlag`).
    pub fn move_skill_set(&mut self, from: usize, to: usize) -> Result<(), String> {
        self.edit_skills(|t, _| {
            let s = t.skill_sets.remove(from - 1);
            t.skill_sets.insert(to - 1, s);
            Ok(())
        })
    }

    // --- undo ------------------------------------------------------------------------------------------------------------------------

    /// `CreateUndoState`.
    pub fn skills_undo_state(&self) -> SkillsUndoState {
        let t = self.skills_tab();
        SkillsUndoState {
            skill_sets: t.skill_sets.clone(),
            active_skill_set_id: t.active_skill_set_id,
            active_socket_group: self.main_socket_group(),
            active_socket_group2: self.calcs_input().get("skill_number").cloned(),
        }
    }

    /// `ResetUndo` at the end of `Load` (SkillsTab.lua:570): CalcSetup has not added its groups yet.
    pub fn skills_load_undo_state(&self) -> Result<SkillsUndoState, String> {
        let skills = self.skill_data();
        let t = pob_calc::load_skills_tab(&self.model().skills, &skills).map_err(lua_err)?;
        Ok(SkillsUndoState {
            skill_sets: t.skill_sets,
            active_skill_set_id: t.active_skill_set_id,
            active_socket_group: self.model().main_socket_group,
            active_socket_group2: self.calcs_input().get("skill_number").cloned(),
        })
    }

    /// `RestoreUndoState` (SkillsTab.lua:1421-1439) without its controls.
    pub fn restore_skills_undo_state(&mut self, state: &SkillsUndoState) -> Result<(), String> {
        self.edit_skills(|t, _| {
            t.skill_sets.clone_from(&state.skill_sets);
            t.active_skill_set_id = state.active_skill_set_id;
            rebuild_imbued(t);
            Ok(())
        })?;
        self.set_main_pending(state.active_socket_group);
        match &state.active_socket_group2 {
            Some(InputValue::Num(n)) => self.set_calcs_skill_number(*n),
            _ => self.clear_calcs_skill_number(),
        }
        Ok(())
    }

    // --- spectres --------------------------------------------------------------------------------------------------------------------

    /// `self.spectreList = destList` (Build.lua:1539).
    pub fn set_spectre_list(&mut self, list: Vec<String>) {
        self.model_mut().spectres = list;
    }

    // --- what the tooltips and `GemSelectControl` calculate ---------------------------------------------------------------------------

    /// The model with `gemList[gem]` of group `group` edited as the tab does it for one `calcFunc()` call.
    fn model_with_edit(&self, group: usize, gem: usize, edit: &GemEdit) -> Result<pob_build::BuildModel, String> {
        let mut m = self.model().clone();
        if self.model_has_runtime_groups() {
            let tab = self.merged_skills_tab()?;
            m.skills.sets = tab
                .skill_sets
                .iter()
                .map(|s| SkillSetModel {
                    id: s.id,
                    title: s.title.clone(),
                    groups: s.socket_group_list.iter().map(group_to_model).collect(),
                })
                .collect();
        }
        let default_level = self.skills_tab().default_gem_level.clone();
        let default_quality = self.skills_tab().default_gem_quality;
        let level = self.model().level;
        let skills = self.skill_data();
        let set = active_skill_set_mut(&mut m).ok_or("no active skill set")?;
        let g = set
            .groups
            .get_mut(group.wrapping_sub(1))
            .ok_or_else(|| format!("SkillsTab: no socket group {group}"))?;
        match edit {
            GemEdit::Candidate { gem_id, imbued } => {
                let gd = pob_data::gems::gems()
                    .gems
                    .0
                    .get(gem_id)
                    .ok_or_else(|| format!("GemSelectControl: no data.gems['{gem_id}']"))?;
                let lv = process_gem_level(gd, *imbued, &default_level, Some(level), &skills).map_err(lua_err)?;
                if g.gems.get(gem.wrapping_sub(1)).is_none() {
                    g.gems.push(GemModel {
                        name_spec: gd.name.clone(),
                        gem_id: None,
                        variant_id: None,
                        skill_id: None,
                        level: Some(lv),
                        quality: Some(default_quality),
                        quality_id: None,
                        enabled: true,
                        enable_global1: true,
                        enable_global2: true,
                        count: 1.0,
                        skill_part: None,
                        skill_part_calcs: None,
                        skill_stage_count: None,
                        skill_stage_count_calcs: None,
                        skill_mine_count: None,
                        skill_mine_count_calcs: None,
                        skill_minion: None,
                        skill_minion_calcs: None,
                        skill_minion_item_set: None,
                        skill_minion_item_set_calcs: None,
                        skill_minion_skill: None,
                        skill_minion_skill_calcs: None,
                    });
                }
                let i = gem.min(g.gems.len()) - 1;
                let gm = &mut g.gems[i];
                gm.name_spec.clone_from(&gd.name);
                gm.gem_id = Some(gd.game_id.clone());
                gm.variant_id = Some(gd.variant_id.clone());
                gm.skill_id = Some(gd.granted_effect_id.clone());
                gm.level = Some(lv);
            }
            GemEdit::Quality20 | GemEdit::ToggleEnabled => {
                let gm = g
                    .gems
                    .get_mut(gem.wrapping_sub(1))
                    .ok_or_else(|| format!("SkillsTab: socket group {group} has no gem {gem}"))?;
                if *edit == GemEdit::Quality20 {
                    gm.quality = Some(20.0);
                } else {
                    gm.enabled = !gm.enabled;
                }
            }
        }
        Ok(m)
    }

    /// The write of `edit` as the calculator's override, or `None` for a group the override cannot name: one with a `source` (no gem slots upstream, SkillsTab.lua:777).
    fn gem_override(&self, group: usize, gem: usize, edit: &GemEdit) -> Result<Option<GemOverride>, String> {
        let groups = self.socket_groups();
        let target = group.wrapping_sub(1);
        let Some(g) = groups.get(target) else {
            return Err(format!("SkillsTab: no socket group {group}"));
        };
        if g.source.is_some() {
            return Ok(None);
        }
        let edit = match edit {
            GemEdit::Candidate { gem_id, imbued } => {
                let gd = pob_data::gems::gems()
                    .gems
                    .0
                    .get(gem_id)
                    .ok_or_else(|| format!("GemSelectControl: no data.gems['{gem_id}']"))?;
                let skills = self.skill_data();
                let tab = self.skills_tab();
                let level = process_gem_level(gd, *imbued, &tab.default_gem_level, Some(self.model().level), &skills).map_err(lua_err)?;
                pob_calc::env::GemEdit::Candidate {
                    gem_data: gem_id.clone(),
                    level,
                    default_quality: tab.default_gem_quality,
                }
            }
            GemEdit::Quality20 => pob_calc::env::GemEdit::Quality(20.0),
            GemEdit::ToggleEnabled => pob_calc::env::GemEdit::ToggleEnabled,
        };
        Ok(Some(GemOverride {
            group: groups[..target].iter().filter(|g| g.source.is_none()).count(),
            gem: gem.wrapping_sub(1),
            edit,
        }))
    }

    /// `calcFunc(nil, useFullDPS)` with the edit (GemSelectControl.lua:96-109, SkillsTab.lua:940-943, :980-982): in the session of `GetMiscCalculator`, over its `GlobalCache` (R47) -- what
    /// earlier calls of this `outputRevision` cached stays there, as upstream. An edit made since the last run (the session does not hold it yet) goes through a session of its own.
    pub fn gem_edit_run(&self, group: usize, gem: usize, edit: &GemEdit, use_full_dps: Option<bool>) -> Result<RunRef, String> {
        if self.skills_live_ref().is_some() {
            return self.gem_edit_run_own_session(group, gem, edit, use_full_dps);
        }
        let Some(ov) = self.gem_override(group, gem, edit)? else {
            return self.gem_edit_run_own_session(group, gem, edit, use_full_dps);
        };
        let calc = self.misc_calculator()?;
        let ov = CalcOverride {
            gem_edit: Some(ov),
            ..CalcOverride::default()
        };
        calc.calc(&Rc::new(ov), use_full_dps, self.view_mode_is_tree.get()).map_err(|e| e.0)
    }

    /// [`Engine::gem_edit_run`] as a session over the edited model: an empty cache, and the gem as `Load` makes it. Costs a full `BuildOutput` per call.
    pub fn gem_edit_run_own_session(&self, group: usize, gem: usize, edit: &GemEdit, use_full_dps: Option<bool>) -> Result<RunRef, String> {
        let m = self.model_with_edit(group, gem, edit)?;
        let sess = self.calc_session(m);
        sess.build_output(pob_calc::session::CalcMode::Main).map_err(|e| e.0)?;
        let calc = sess.get_misc_calculator().map_err(|e| e.0)?;
        let run = calc
            .calc(
                &Rc::new(pob_calc::env::CalcOverride::default()),
                use_full_dps,
                self.view_mode_is_tree.get(),
            )
            .map_err(|e| e.0)?;
        if matches!(edit, GemEdit::Candidate { .. }) {
            // `CalcOutputWithThisGem` writes `gemData` and `level`, not `reqStr` / `reqDex` / `reqInt` (ProcessSocketGroup's): the gem's old ones (nil in an empty slot) go on to
            // `env.requirementsTableGems`, the hovered gem never moves the `Req*` lines
            let own = self.misc_calculator()?;
            let base = own.base.borrow();
            let mut r = run.borrow_mut();
            for k in ["ReqStr", "ReqDex", "ReqInt", "ReqOmni"] {
                match base.state.player.output.opt_num(k) {
                    Some(v) => r.state.player.output.set_num(k, v),
                    None => r.state.player.output.remove(k),
                }
            }
        }
        Ok(run)
    }

    /// `group.slotEnabled` of the groups a `calcFunc` has seen: every `initEnv` writes it on the live tables (CalcSetup.lua:1600), the groups made since the last run included.
    pub fn mark_slot_enabled(&mut self) {
        let want = if pob_build::active_item_set(self.model()).use_second_weapon_set {
            2.0
        } else {
            1.0
        };
        let slots: Vec<(String, Option<f64>)> = self
            .result()
            .env
            .ordered_slots
            .iter()
            .map(|s| (s.slot_name.clone(), s.weapon_set))
            .collect();
        if let Some(tab) = self.skills_live_mut() {
            for g in tab.socket_group_list_mut() {
                if g.slot_enabled.is_none() {
                    let set = g.slot.as_ref().and_then(|n| slots.iter().find(|s| s.0 == *n)).and_then(|s| s.1);
                    g.slot_enabled = Some(set.is_none_or(|x| x == want));
                }
            }
        }
    }

    /// `baseDPS` of `UpdateSortCache` (GemSelectControl.lua:352-354).
    pub fn gem_base_dps(&self, field: &str) -> Result<f64, String> {
        let calc = self.misc_calculator()?;
        Ok(dps_of(&calc.base, field))
    }

    /// One step of `DPSBuilder` (GemSelectControl.lua:429-430).
    pub fn gem_candidate_dps(&self, group: usize, gem: usize, gem_id: &str, imbued: bool, field: &str) -> Result<f64, String> {
        let edit = GemEdit::Candidate {
            gem_id: gem_id.to_owned(),
            imbued,
        };
        let run = self.gem_edit_run(group, gem, &edit, Some(field == "FullDPS"))?;
        Ok(dps_of(&run, field))
    }

    /// `AddStatComparesToTooltip(tooltip, calcBase, output, header)` of a gem edit: the lines for `AddLine(14, line)`.
    pub fn gem_stat_compare(
        &self,
        group: usize,
        gem: usize,
        edit: &GemEdit,
        header: &str,
        use_full_dps: Option<bool>,
    ) -> Result<Vec<String>, String> {
        let run = self.gem_edit_run(group, gem, edit, use_full_dps)?;
        let calc = self.misc_calculator()?;
        self.stat_compare_lines(&calc.base, &run, header)
    }

    /// `activeSkill` as `canGrantedEffectSupportActiveSkill` reads it, for one skill of `displaySkillList`.
    fn can_support_skill(env: &Env, ge: &pob_data::skills::GrantedEffect, si: usize, imbued: bool) -> Result<bool, String> {
        let st = env.skills.as_ref().expect("skills on");
        let sk = st.skill(si);
        let (act_idx, has_gem, src) = {
            let a = sk.active_effect.borrow();
            (a.granted_effect, a.gem_data.is_some(), a.src_instance)
        };
        let act = &st.skills.effects[act_idx];
        let src_from_item = src
            .and_then(|(g, k)| st.tab.socket_group_list().get(g).and_then(|g| g.gem_list.get(k)))
            .is_some_and(|g| g.from_item == Some(true));
        let p = env.player.borrow();
        let w1 = p.weapon_data1.get("type");
        let w2 = p.weapon_data2.get("type");
        let view = ActiveSkillView {
            granted_effect: act,
            has_gem_data: has_gem,
            src_instance_from_item: src_from_item,
            skill_types: Some(&sk.skill_types),
            minion_skill_types: sk.minion_skill_types.as_ref(),
            summon: None,
            actor_is_player: true,
            weapon1: WeaponView {
                ty: w1.as_str(),
                counts_as_all_1h: p.weapon_data1.get("countsAsAll1H").truthy(),
            },
            weapon2: WeaponView {
                ty: w2.as_str(),
                counts_as_all_1h: p.weapon_data2.get("countsAsAll1H").truthy(),
            },
        };
        let from_item = |g: &pob_data::skills::GrantedEffect| g.from_item || st.from_item_marks.contains(&g.id);
        can_granted_effect_support_active_skill_ext(ge, &view, imbued, false, from_item(ge), from_item(act)).map_err(lua_err)
    }

    /// `sortCache.canSupport` of `UpdateSortCache` (GemSelectControl.lua:305-348): which of `gems` (ids) support what group `group` holds.
    pub fn gem_can_support(&self, group: usize, gems: &[&str], imbued: bool) -> Result<std::collections::HashSet<String>, String> {
        let env = &self.result().env;
        let data = pob_data::gems::gems();
        let st = env.skills.as_ref().expect("skills on");
        let skills = &st.skills;
        let mut out = std::collections::HashSet::new();
        let list = self.socket_groups();
        let Some(g) = list.get(group.wrapping_sub(1)) else { return Ok(out) };
        let mut add = |sl: &[usize]| -> Result<(), String> {
            for id in gems {
                let Some(ge) = data.gems.0.get(*id).and_then(|d| skills.get(&d.granted_effect_id)) else {
                    continue;
                };
                if ge.support {
                    for &si in sl {
                        if Self::can_support_skill(env, ge, si, imbued)? {
                            out.insert((*id).to_owned());
                            break;
                        }
                    }
                }
            }
            Ok(())
        };
        if let Some(dl) = g.display_skill_list.as_ref().filter(|d| !d.is_empty()) {
            add(dl)?;
        } else if let Some(slot) = &g.slot {
            let strip = |s: &str| s.replace(" Swap", "");
            for other in list {
                let Some(dl) = other.display_skill_list.as_ref().filter(|d| !d.is_empty()) else {
                    continue;
                };
                let Some(oslot) = &other.slot else { continue };
                if other.source.is_some() && oslot == slot {
                    add(dl)?;
                }
                for cross in st.cross_linked_support_groups.get(&strip(slot)).into_iter().flatten() {
                    if *cross == strip(oslot) {
                        add(dl)?;
                    }
                }
            }
        }
        Ok(out)
    }
}

impl Engine {
    /// `build.itemsTab.items[slots[name].selItemId]`.
    pub fn slot_item(&self, name: &str) -> Option<Rc<pob_items::Item>> {
        let env = &self.result().env;
        let slot = env.ordered_slots.iter().find(|s| s.slot_name == name)?;
        env.item(slot.sel_item_id).cloned()
    }

    /// `build:AddRequirementsToTooltip(tooltip, level, str, dex, int)` (Build.lua:2185-2219): the `Requires` line, when there is one.
    pub fn requirements_line(&self, level: f64, str_: f64, dex: f64, int: f64) -> Option<String> {
        use pob_lua::format::{Arg, format};
        let env = &self.result().env;
        let o = &self.result().perform.player.output;
        let col = |stat: f64, limit: f64| {
            if stat > limit {
                pob_data::globals::globals()
                    .color_codes
                    .get("NEGATIVE")
                    .cloned()
                    .unwrap_or_default()
            } else {
                "^7".to_owned()
            }
        };
        let f = |spec: &str, c: &str, v: f64| format(spec, &[Arg::Str(c), Arg::Num(v)]).expect("format");
        let mut req: Vec<String> = Vec::new();
        if level > 0.0 {
            req.push(f("^x7F7F7FLevel %s%d", &col(level, self.model().level), level));
        }
        let db = env.mod_db.borrow();
        if db.flag(None, &["OmniscienceRequirements"]).ok().flatten() == Some(true) {
            let satisfy = db.sum(pob_mods::ModType::Inc, None, &["OmniAttributeRequirements"]).unwrap_or(0.0);
            let highest = str_.max(dex).max(int).max(0.0);
            let omni = (highest * (100.0 / satisfy)).floor();
            let have = o.opt_num("Omni").unwrap_or(0.0);
            if omni > 0.0 || omni > have {
                req.push(f("%s%d ^x7F7F7FOmni", &col(omni, have), omni));
            }
        } else {
            for (v, name) in [(str_, "Str"), (dex, "Dex"), (int, "Int")] {
                let have = o.opt_num(name).unwrap_or(0.0);
                if v > 14.0 || v > have {
                    req.push(format("%s%d ^x7F7F7F%s", &[Arg::Str(&col(v, have)), Arg::Num(v), Arg::Str(name)]).expect("format"));
                }
            }
        }
        if req.is_empty() {
            return None;
        }
        Some(format!("^x7F7F7FRequires {}", req.join("^x7F7F7F, ")))
    }

    /// `controls.sourceNote.label` (SkillsTab.lua:387-417) of group `index`.
    pub fn source_note(&self, index: usize) -> Result<String, String> {
        use pob_calc::skills_tab::ExplodeSource;
        let code = |n: &str| pob_data::globals::globals().color_codes.get(n).cloned().unwrap_or_default();
        let g = self
            .socket_groups()
            .get(index.wrapping_sub(1))
            .ok_or_else(|| format!("SkillsTab: no socket group {index}"))?;
        let env = &self.result().env;
        let node_name = |id: u32| {
            env.spec.node(id).map_or_else(String::new, |n| {
                let t = n.tree_node();
                t.name.clone().unwrap_or_else(|| t.dn.clone())
            })
        };
        if let Some(list) = &g.explode_sources {
            let mut label =
                "^7This is a special group created for the enemy explosion effect,\nwhich comes from the following sources:".to_owned();
            for s in list {
                let (rarity, name) = match s {
                    ExplodeSource::Node(id) => ("NORMAL".to_owned(), node_name(*id)),
                    ExplodeSource::Item(i) => (i.0.rarity.clone(), i.0.name.clone()),
                };
                label.push_str(&format!("\n\t{}{name}", code(&rarity)));
            }
            label.push_str("^7\nYou cannot delete this group, but it will disappear if you lose the above sources.");
            return Ok(label);
        }
        let active = g
            .gem_list
            .first()
            .ok_or("Classes/SkillsTab.lua:407: attempt to index local 'activeGem' (a nil value)")?;
        let source_name = if let Some(i) = &g.source_item {
            format!("'{}{}", code(&i.0.rarity), i.0.name)
        } else if let Some(n) = g.source_node {
            format!("'{}{}", code("NORMAL"), node_name(n))
        } else {
            format!("'{}?", code("NORMAL"))
        };
        let source_name = format!("{source_name}^7'");
        let skills = pob_data::skills::skills();
        let name = active
            .granted_effect
            .as_deref()
            .and_then(|i| skills.get(i))
            .map(|x| x.name.clone())
            .or_else(|| active.name_spec.clone())
            .unwrap_or_default();
        let mut label = format!(
            "^7This is a special group created for the '{}{name}^7' skill,\nwhich is being provided by {source_name}.\nYou cannot delete this group, but it will disappear if you {}",
            active.color.as_deref().unwrap_or(""),
            if g.source_node.is_some() {
                "un-allocate the node."
            } else {
                "un-equip the item."
            }
        );
        if g.no_supports != Some(true) && g.source_node.is_none() {
            label.push_str(&format!(
                "\n\nYou cannot add support gems to this group, but support gems in\nany other group socketed into {source_name}will automatically apply to the skill."
            ));
        }
        Ok(label)
    }
}

/// The fields of a group the tab or CalcSetup made that upstream leaves nil and the model cannot: `SaveDB` writes `"nil"` for them. `gems`: `count`, `enableGlobal1`, `enableGlobal2`.
#[derive(Clone, Debug, Default)]
pub struct NilGroup {
    pub main: bool,
    pub calcs: bool,
    pub gems: Vec<(bool, bool, bool)>,
}

/// Which groups have `mainActiveSkill` / `mainActiveSkillCalcs` nil, by set id.
pub(crate) fn nil_main_of(tab: &SkillsTab) -> std::collections::HashMap<u64, Vec<NilGroup>> {
    tab.skill_sets
        .iter()
        .map(|s| {
            (
                s.id as u64,
                s.socket_group_list
                    .iter()
                    .map(|g| NilGroup {
                        main: g.main_active_skill.is_none(),
                        calcs: g.main_active_skill_calcs.is_none(),
                        gems: g
                            .gem_list
                            .iter()
                            .map(|x| (x.count.is_none(), x.enable_global1.is_none(), x.enable_global2.is_none()))
                            .collect(),
                    })
                    .collect(),
            )
        })
        .collect()
}

fn first_free_set(t: &SkillsTab) -> f64 {
    let mut id = 1.0;
    while t.skill_sets.iter().any(|s| s.id == id) {
        id += 1.0;
    }
    id
}
