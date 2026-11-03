//! What the Build screen's shell (U2c, `Modules/Build.lua` without the tabs) reads from and writes into the engine:
//! - [`Engine::skill_select`] -- `buildMode:RefreshSkillSelectControls(controls, mainGroup, "")` (Build.lua:1618-1716) as data;
//! - [`point_display`] -- the `pointDisplay.str` / `.req` of `EstimatePlayerProgress` (Build.lua:898-940);
//! - [`Engine::class_select`] -- `UpdateClassDropdowns` (Build.lua:1553-1574) and `UpdateSecondaryAscendancyDropdown` (:1113-1158) as data;
//! - the mutators of the main-skill drop-downs (Build.lua:481-568) and of the level controls (:215-229). Like upstream's control callbacks
//!   they only change the state (`buildFlag = true`): the caller runs [`Engine::refresh`] (`BuildOutput` + `RefreshStatList`, :1258-1271).
//!   Each has a path-equivalence test in `tests/shell.rs`.
//!
//! The reads are over the MAIN run (`calcsTab.mainEnv`); upstream's `displaySkillList` is the one the CALCS run wrote last (`BuildOutput`
//! runs MAIN, then CALCS over the same socket groups) -- the same skills, labels and minions (see the U2c report).

use pob_calc::CalculateResult;
use pob_calc::skills_tab::ExplodeSource;

use crate::engine::Engine;
use crate::stats::color;

/// `controls.pointDisplay.str` / `.req`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointDisplay {
    pub str: String,
    pub req: String,
}

/// Build.lua:729-764 `acts` (the same table as `pob_calc::calculate::ACTS`, which is private to that crate).
const ACTS: [(f64, f64); 11] = [
    (1.0, 0.0),
    (12.0, 2.0),
    (22.0, 4.0),
    (32.0, 6.0),
    (40.0, 7.0),
    (44.0, 9.0),
    (50.0, 12.0),
    (54.0, 15.0),
    (60.0, 18.0),
    (64.0, 20.0),
    (67.0, 23.0),
];

/// Build.lua:766-769
fn act_extra(act: usize, extra: f64) -> f64 {
    if act > 2 { extra } else { 0.0 }
}

/// Build.lua:898-940 `EstimatePlayerProgress` -- the point display (the level and the warnings are `calculate`'s / [`crate::display`]'s).
pub fn point_display(res: &CalculateResult) -> PointDisplay {
    let c = res.env.spec.count_alloc_nodes();
    let points_used = f64::from(c.used);
    let asc_used = f64::from(c.asc_used);
    let extra = res.perform.player.output.opt_num("ExtraPoints").unwrap_or(0.0);
    let (used_max, asc_max) = (99.0 + 23.0 + extra, 8.0);
    let mut act = 0;
    let mut level;
    loop {
        act += 1;
        let (act_level, quest_points) = ACTS[act - 1];
        level = pob_lua::number::min2(
            pob_lua::number::max2(points_used + 1.0 - quest_points - act_extra(act, extra), act_level),
            100.0,
        );
        if act == 11 || level <= ACTS[act].0 {
            break;
        }
    }
    let lab_suggest = if level < 33.0 {
        ""
    } else if level < 55.0 {
        "\nLabyrinth: Normal Lab"
    } else if level < 68.0 {
        "\nLabyrinth: Cruel Lab"
    } else if level < 75.0 {
        "\nLabyrinth: Merciless Lab"
    } else if level < 90.0 {
        "\nLabyrinth: Uber Lab"
    } else {
        ""
    };
    // `self.Act = level < 90 and act <= 10 and act or "Endgame"`
    let act_str = if level < 90.0 && act <= 10 {
        act.to_string()
    } else {
        "Endgame".to_owned()
    };
    let d = |v: f64| format!("{}", v as i64);
    PointDisplay {
        // `string.format("%s%3d / %3d   %s%d / %d", ...)`
        str: format!(
            "{}{:>3} / {:>3}   {}{} / {}",
            if points_used > used_max { color::NEGATIVE } else { "^7" },
            d(points_used),
            d(used_max),
            if asc_used > asc_max { color::NEGATIVE } else { "^7" },
            d(asc_used),
            d(asc_max)
        ),
        req: format!(
            "Required Level: {}\nEstimated Progress:\nAct: {act_str}\nQuestpoints: {}\nExtra Skillpoints: {}{lab_suggest}",
            d(level),
            d(ACTS[act - 1].1),
            d(act_extra(act, extra))
        ),
    }
}

// --- CalcSetup.lua:1009-1020 missingAnointWarning --------------------------------------------------------------------------------------

/// `env.itemWarnings.missingAnointWarning` (CalcSetup.lua:1009-1020, `ItemsTab:getMissingAnointCount` ItemsTab.lua:2978-3005,
/// `isAnointable` :70-74), which pob-calc leaves out (`init_env.rs`: "UI only"): the slot labels of the equipped items (after the
/// disablers: `env.player.itemList`) with fewer anoints than they can hold.
pub fn missing_anoint_warning(res: &CalculateResult) -> Option<Vec<String>> {
    let env = &res.env;
    let player = env.player.borrow();
    let mut out: Option<Vec<String>> = None;
    for slot in &env.ordered_slots {
        let Some(item) = player.item_list.get(&slot.slot_name) else {
            continue;
        };
        let Some(base) = item.base else { continue };
        let anointable = base.cannot_be_anointed != Some(true)
            && base.sub_type.as_deref() != Some("Talisman")
            && (item.can_be_anointed || base.ty == "Amulet");
        if !anointable {
            continue;
        }
        let max: usize = if item.can_have_four_enchants {
            4
        } else if item.can_have_three_enchants {
            3
        } else if item.can_have_two_enchants {
            2
        } else {
            1
        };
        // `line:find("Allocates ([a-zA-Z ]+)")`
        let count = [
            &item.enchant_mod_lines,
            &item.scourge_mod_lines,
            &item.implicit_mod_lines,
            &item.explicit_mod_lines,
            &item.crucible_mod_lines,
        ]
        .iter()
        .flat_map(|l| l.iter())
        .filter(|m| {
            pob_lua::find(m.line.as_bytes(), b"Allocates ([a-zA-Z ]+)", None, false)
                .ok()
                .flatten()
                .is_some()
        })
        .count();
        let missing = max - count.min(max);
        if missing > 0 {
            let label = if missing > 1 {
                format!("{} ({missing} missing)", slot.label)
            } else {
                slot.label.clone()
            };
            out.get_or_insert_with(Vec::new).push(label);
        }
    }
    out
}

// --- RefreshSkillSelectControls ------------------------------------------------------------------------------------------------------

/// A `controls.mainSkill.list` label: `"From "..colorCodes[rarity or "NORMAL"]..name` for an exploding source, else the effect name.
#[derive(Clone, Debug, PartialEq)]
pub enum SkillLabel {
    Name(String),
    /// `(source name, rarity)` -- the UI picks the colour (`colorCodes` may be user-edited).
    From(String, Option<String>),
}

/// An entry of `controls.mainSkillMinion.list`.
#[derive(Clone, Debug, PartialEq)]
pub enum MinionEntry {
    /// `{ label = itemSet.title or "Default Item Set", itemSetId }`
    ItemSet { label: String, item_set_id: f64 },
    /// `{ label = data.minions[minionId].name, minionId }`
    Minion { label: String, minion_id: String },
    /// The bare string `"<No spectres in build>"` (Build.lua:1710).
    Text(String),
}

/// `controls.mainSkillMinion:SelByValue(value, key)`: `None` = the value matches no entry (upstream passes `list[1]`, a table, as the
/// value of the `minionId` lookup when `skillMinion` is nil -- nothing is selected, `selIndex` stays as it was).
#[derive(Clone, Debug, PartialEq)]
pub enum MinionSel {
    ItemSet(f64),
    Minion(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinionSelect {
    pub list: Vec<MinionEntry>,
    pub sel: MinionSel,
    /// `#list > 1`, taken before the `"<No spectres in build>"` entry is added.
    pub enabled: bool,
    pub library_shown: bool,
    /// `controls.mainSkillMinionSkill`: the names, `selIndex` (shown only with `activeSkill.minion`).
    pub skills: Option<(Vec<String>, f64)>,
}

/// The controls under `mainSocketGroup` when the group has skills.
#[derive(Clone, Debug, PartialEq)]
pub struct MainSkillSelect {
    pub skills: Vec<SkillLabel>,
    pub skill_sel: f64,
    /// `mainSkillPart`: names + `selIndex`.
    pub part: Option<(Vec<String>, f64)>,
    /// `mainSkillStageCount.buf` when shown.
    pub stage_count: Option<String>,
    /// `mainSkillMineCount.buf` when shown.
    pub mine_count: Option<String>,
    pub minion: Option<MinionSelect>,
}

/// `RefreshSkillSelectControls` as data.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillSelect {
    /// `controls.mainSocketGroup.selIndex = mainGroup`.
    pub main_group: f64,
    /// `socketGroup.displayLabel` of every group; empty = `"<No skills added yet>"` and the rest hidden.
    pub groups: Vec<String>,
    /// `None` when there are no groups; `Some` with `skills` empty when the group has no active skill.
    pub main: Option<MainSkillSelect>,
}

/// `data.minions[id].name`.
fn minion_name(id: &str) -> Result<String, String> {
    thread_local! {
        static M: std::cell::OnceCell<serde_json::Value> = const { std::cell::OnceCell::new() };
    }
    M.with(|c| {
        let j = c.get_or_init(|| pob_data::read_json("minions.json").unwrap_or_else(|e| panic!("pob-data: minions.json: {e}")));
        j["minions"][id]["name"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("Modules/Build.lua:1693: attempt to index field '?' (data.minions['{id}'] is nil)"))
    })
}

fn tostring(v: f64) -> String {
    pob_lua::tostring_num(v)
}

/// The value of a `skillData` key as a number (`nil` otherwise).
fn skill_data_num(sd: &indexmap::IndexMap<String, pob_mods::Value>, k: &str) -> Option<f64> {
    sd.get(k).and_then(pob_mods::Value::as_num)
}

impl Engine {
    /// `build.mainSocketGroup` as the run left it (CalcSetup.lua:1586-1590 clamps it).
    pub fn main_socket_group(&self) -> f64 {
        if let Some(v) = self.main_pending() {
            return v;
        }
        self.result()
            .env
            .skills
            .as_ref()
            .map_or(self.model().main_socket_group, |s| s.build_main_socket_group)
    }

    /// Build.lua:1618-1716 `RefreshSkillSelectControls(controls, self.mainSocketGroup, "")`.
    pub fn skill_select(&self) -> Result<SkillSelect, String> {
        self.skill_select_in(&self.result().env, self.main_socket_group(), false)
    }

    /// `RefreshSkillSelectControls(section.controls, self.input.skill_number, "Calcs")` (CalcsTab.lua:145): the `*Calcs` fields of the CALCS run.
    pub fn skill_select_calcs(&self) -> Result<SkillSelect, String> {
        let main_group = match self.calcs_input().get("skill_number") {
            Some(pob_calc::pob_build::InputValue::Num(n)) => *n,
            _ => 1.0,
        };
        self.skill_select_in(&self.calcs_run().env, main_group, true)
    }

    fn skill_select_in(&self, env: &pob_calc::Env, main_group: f64, calcs: bool) -> Result<SkillSelect, String> {
        let Some(st) = env.skills.as_ref() else {
            return Ok(SkillSelect {
                main_group,
                groups: Vec::new(),
                main: None,
            });
        };
        let list = st.tab.socket_group_list();
        let groups: Vec<String> = list
            .iter()
            .map(|g| {
                g.display_label
                    .clone()
                    .ok_or("Classes/DropDownControl.lua: label of a socket group is nil")
            })
            .collect::<Result<_, _>>()?;
        if groups.is_empty() {
            return Ok(SkillSelect {
                main_group,
                groups,
                main: None,
            });
        }
        // :1635 `self.skillsTab.socketGroupList[mainGroup]`
        let gi = (main_group.fract() == 0.0 && main_group >= 1.0 && main_group as usize <= list.len())
            .then(|| main_group as usize - 1)
            .ok_or("Modules/Build.lua:1636: attempt to index local 'mainSocketGroup' (a nil value)")?;
        let group = &list[gi];
        let display = group
            .display_skill_list
            .as_ref()
            .ok_or("Modules/Build.lua:1639: bad argument #1 to 'ipairs' (table expected, got nil)")?;
        let main_active = if calcs {
            group.main_active_skill_calcs
        } else {
            group.main_active_skill
        }
        .unwrap_or(1.0);
        let skills_data = &st.skills;
        let mut labels = Vec::new();
        for &si in display {
            let s = st.skill(si);
            let ae = s.active_effect.borrow();
            let explode = ae
                .src_instance
                .and_then(|(g, k)| list.get(g).and_then(|g| g.gem_list.get(k)))
                .and_then(|gem| gem.explode_source.clone());
            labels.push(match explode {
                Some(ExplodeSource::Node(id)) => {
                    let n = env.spec.node(id).ok_or("explodeSource: unknown node")?;
                    let tn = n.tree_node();
                    SkillLabel::From(tn.name.clone().unwrap_or_else(|| tn.dn.clone()), None)
                }
                Some(ExplodeSource::Item(i)) => SkillLabel::From(i.0.name.clone(), Some(i.0.rarity.clone())),
                None => SkillLabel::Name(skills_data.effects[ae.granted_effect].name.clone()),
            });
        }
        let mut sel = MainSkillSelect {
            skills: labels,
            skill_sel: main_active,
            part: None,
            stage_count: None,
            mine_count: None,
            minion: None,
        };
        if !display.is_empty() {
            // :1655 `displaySkillList[mainActiveSkill]`
            let si = (main_active.fract() == 0.0 && main_active >= 1.0 && main_active as usize <= display.len())
                .then(|| display[main_active as usize - 1])
                .ok_or("Modules/Build.lua:1656: attempt to index local 'activeSkill' (a nil value)")?;
            let active = st.skill(si);
            let ae = active.active_effect.borrow();
            let ge = &skills_data.effects[ae.granted_effect];
            let src = ae.src_instance.and_then(|(g, k)| list.get(g).and_then(|g| g.gem_list.get(k)));
            let src_err = || "Modules/Build.lua: attempt to index field 'srcInstance' (a nil value)".to_owned();
            let flag = |k: &str| active.skill_flags.get(k).copied().unwrap_or(false);
            let multi_part = ge.parts.as_ref().is_some_and(|p| p.len() > 1);
            if let Some(parts) = ge.parts.as_ref().filter(|p| p.len() > 1) {
                let src = src.ok_or_else(src_err)?;
                let part_sel = if calcs { src.skill_part_calcs } else { src.skill_part }.unwrap_or(1.0);
                let part = (part_sel.fract() == 0.0 && part_sel >= 1.0 && part_sel as usize <= parts.len())
                    .then(|| &parts[part_sel as usize - 1])
                    .ok_or("Modules/Build.lua:1665: attempt to index field '?' (a nil value)")?;
                if part.stages == Some(true) {
                    // `stagesMin` is not a field of any part in the data
                    sel.stage_count = Some(tostring(
                        (if calcs {
                            src.skill_stage_count_calcs
                        } else {
                            src.skill_stage_count
                        })
                        .or_else(|| skill_data_num(&active.skill_data, "stagesMax"))
                        .unwrap_or(1.0),
                    ));
                }
                sel.part = Some((parts.iter().map(|p| p.name.clone()).collect(), part_sel));
            }
            if flag("mine") {
                let src = src.ok_or_else(src_err)?;
                sel.mine_count =
                    Some(if calcs { src.skill_mine_count_calcs } else { src.skill_mine_count }.map_or_else(String::new, tostring));
            }
            if flag("multiStage") && !multi_part {
                let src = src.ok_or_else(src_err)?;
                sel.stage_count = Some(tostring(
                    (if calcs {
                        src.skill_stage_count_calcs
                    } else {
                        src.skill_stage_count
                    })
                    .or_else(|| skill_data_num(&active.skill_data, "stagesMax"))
                    .or_else(|| skill_data_num(&active.skill_data, "stagesMin"))
                    .unwrap_or(1.0),
                ));
            }
            let skill_minion_list = active.minion_list.as_deref().unwrap_or(&[]);
            if !flag("disable") && (ge.minion_list.is_some() || !skill_minion_list.is_empty()) {
                let src = src.ok_or_else(src_err)?;
                let mut mlist = Vec::new();
                let mut library_shown = false;
                let msel = if ge.minion_has_item_set {
                    let m = self.model();
                    for set in &m.item_sets {
                        mlist.push(MinionEntry::ItemSet {
                            label: set.title.clone().unwrap_or_else(|| "Default Item Set".into()),
                            item_set_id: set.id,
                        });
                    }
                    MinionSel::ItemSet(
                        if calcs {
                            src.skill_minion_item_set_calcs
                        } else {
                            src.skill_minion_item_set
                        }
                        .unwrap_or(1.0),
                    )
                } else {
                    library_shown = ge.minion_list.as_ref().is_some_and(Vec::is_empty);
                    for id in skill_minion_list {
                        mlist.push(MinionEntry::Minion {
                            label: minion_name(id)?,
                            minion_id: id.clone(),
                        });
                    }
                    MinionSel::Minion(if calcs {
                        src.skill_minion_calcs.clone()
                    } else {
                        src.skill_minion.clone()
                    })
                };
                let enabled = mlist.len() > 1;
                let skills = match active.minion.as_deref() {
                    Some(minion) => Some((
                        minion
                            .active_skill_list
                            .iter()
                            .map(|s| skills_data.effects[s.active_effect.borrow().granted_effect].name.clone())
                            .collect(),
                        if calcs {
                            src.skill_minion_skill_calcs
                        } else {
                            src.skill_minion_skill
                        }
                        .unwrap_or(1.0),
                    )),
                    None => {
                        mlist.push(MinionEntry::Text("<No spectres in build>".into()));
                        None
                    }
                };
                sel.minion = Some(MinionSelect {
                    list: mlist,
                    sel: msel,
                    enabled,
                    library_shown,
                    skills,
                });
            }
        }
        Ok(SkillSelect {
            main_group,
            groups,
            main: Some(sel),
        })
    }

    // --- mutators (no recalc: the caller sets `buildFlag`) ---------------------------------------------------------------------------

    /// The `<Gem>` of the model behind `displaySkillList[mainActiveSkill].activeEffect.srcInstance` of the main group (Build.lua:500-501),
    /// after writing back CalcSetup's item groups (as [`Engine::set_gem`]).
    fn main_src_gem(&mut self) -> Result<&mut pob_calc::pob_build::GemModel, String> {
        let (g, k) = {
            let res = self.result();
            let st = res.env.skills.as_ref().ok_or("no skills")?;
            let list = st.tab.socket_group_list();
            let mg = self.main_socket_group();
            let group = list
                .get((mg as usize).wrapping_sub(1))
                .ok_or("Modules/Build.lua:501: attempt to index local 'mainSocketGroup' (a nil value)")?;
            let display = group.display_skill_list.as_ref().ok_or("displaySkillList is nil")?;
            let mas = group.main_active_skill.unwrap_or(1.0);
            let si = *display
                .get((mas as usize).wrapping_sub(1))
                .ok_or("Modules/Build.lua:501: attempt to index field '?' (a nil value)")?;
            st.skill(si)
                .active_effect
                .borrow()
                .src_instance
                .ok_or("Modules/Build.lua:501: attempt to index field 'srcInstance' (a nil value)")?
        };
        if self.model_has_runtime_groups() {
            self.materialize()?;
        }
        let set = crate::engine::active_skill_set_mut(self.model_mut()).ok_or("no active skill set")?;
        set.groups
            .get_mut(g)
            .and_then(|grp| grp.gems.get_mut(k))
            .ok_or_else(|| format!("socket group {} gem {} is not in the saved state", g + 1, k + 1))
    }

    /// `controls.mainSocketGroup` selFunc (Build.lua:481-485): `self.mainSocketGroup = index`.
    pub fn select_main_socket_group(&mut self, index: f64) {
        self.model_mut().main_socket_group = index;
    }

    /// `controls.mainSkill` selFunc (Build.lua:493-498): `socketGroupList[mainSocketGroup].mainActiveSkill = index`.
    pub fn set_main_active_skill(&mut self, index: f64) -> Result<(), String> {
        let g = self.main_socket_group() as usize;
        if self.model_has_runtime_groups() {
            self.materialize()?;
        }
        let set = crate::engine::active_skill_set_mut(self.model_mut()).ok_or("no active skill set")?;
        let grp = set
            .groups
            .get_mut(g.wrapping_sub(1))
            .ok_or("Modules/Build.lua:495: attempt to index local 'mainSocketGroup' (a nil value)")?;
        grp.main_active_skill = index;
        Ok(())
    }

    /// `controls.mainSkillPart` selFunc (Build.lua:499-505): `srcInstance.skillPart = index`.
    pub fn set_skill_part(&mut self, index: f64) -> Result<(), String> {
        self.main_src_gem()?.skill_part = Some(index);
        Ok(())
    }

    /// `controls.mainSkillStageCount` changeFunc (Build.lua:511-517): `srcInstance.skillStageCount = tonumber(buf)`.
    pub fn set_skill_stage_count(&mut self, count: Option<f64>) -> Result<(), String> {
        self.main_src_gem()?.skill_stage_count = count;
        Ok(())
    }

    /// `controls.mainSkillMineCount` changeFunc (Build.lua:523-529): `srcInstance.skillMineCount = tonumber(buf)`.
    pub fn set_skill_mine_count(&mut self, count: Option<f64>) -> Result<(), String> {
        self.main_src_gem()?.skill_mine_count = count;
        Ok(())
    }

    /// `controls.mainSkillMinion` selFunc (Build.lua:530-540): `skillMinionItemSet = value.itemSetId` or `skillMinion = value.minionId`.
    pub fn set_skill_minion(&mut self, value: &MinionEntry) -> Result<(), String> {
        let gem = self.main_src_gem()?;
        match value {
            MinionEntry::ItemSet { item_set_id, .. } => gem.skill_minion_item_set = Some(*item_set_id),
            MinionEntry::Minion { minion_id, .. } => gem.skill_minion = Some(minion_id.clone()),
            // `value.itemSetId` / `value.minionId` of a string: nil -> `skillMinion = nil`
            MinionEntry::Text(_) => gem.skill_minion = None,
        }
        Ok(())
    }

    /// `controls.mainSkillMinionSkill` selFunc (Build.lua:562-568): `srcInstance.skillMinionSkill = index`.
    pub fn set_skill_minion_skill(&mut self, index: f64) -> Result<(), String> {
        self.main_src_gem()?.skill_minion_skill = Some(index);
        Ok(())
    }

    /// `controls.characterLevel` changeFunc (Build.lua:222-229): `characterLevel = m_min(m_max(tonumber(buf) or 1, 1), 100)`, manual mode.
    pub fn set_character_level(&mut self, level: Option<f64>) {
        let m = self.model_mut();
        m.level = pob_lua::number::min2(pob_lua::number::max2(level.unwrap_or(1.0), 1.0), 100.0);
        m.character_level_auto_mode = false;
    }

    /// `controls.levelScalingButton` onClick (Build.lua:215-221): toggles `characterLevelAutoMode`.
    pub fn set_character_level_auto_mode(&mut self, auto: bool) {
        self.model_mut().character_level_auto_mode = auto;
    }

    /// `UpdateClassDropdowns` + the selections `OnFrame` makes (Build.lua:1252-1256, 1553-1574, 1113-1158) over the spec's tree.
    pub fn class_select(&self) -> ClassSelect {
        let spec = &self.result().env.spec;
        let tree = &spec.tree;
        let mut classes: Vec<ClassEntry> = tree
            .classes
            .iter()
            .enumerate()
            .map(|(class_id, c)| ClassEntry {
                label: c.name.clone(),
                class_id,
                ascendancies: c.classes.iter().map(|a| a.name.clone()).collect(),
            })
            .collect();
        // `table.sort(list, function(a, b) return a.label < b.label end)` -- the labels are distinct
        classes.sort_by(|a, b| a.label.as_bytes().cmp(b.label.as_bytes()));
        let secondary = spec.cur_secondary_ascend_class_id.unwrap_or(0.0);
        // PassiveTree.lua:132-176 drops the legacy ones; Build.lua:1136-1149 keeps a legacy one only when it is selected (never: it is gone)
        const LEGACY: [&str; 3] = ["Warden", "Warlock", "Primalist"];
        let mut alt: Vec<(String, f64)> = Vec::new();
        if let Some(serde_json::Value::Array(a)) = tree.json.alternate_ascendancies.as_ref() {
            for (i, asc) in a.iter().enumerate() {
                let id = asc["id"].as_str().unwrap_or("");
                if !LEGACY.contains(&id) && !id.is_empty() {
                    alt.push((asc["name"].as_str().unwrap_or("").to_owned(), (i + 1) as f64));
                }
            }
        }
        alt.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut secondary_list = vec![("None".to_owned(), 0.0)];
        secondary_list.extend(alt);
        ClassSelect {
            classes,
            class_id: spec.cur_class_id,
            ascend_class_id: spec.cur_ascend_class_id,
            secondary_list,
            secondary_ascend_class_id: secondary,
            title_class: {
                let mut t = if spec.cur_ascend_class_id == Some(0) {
                    spec.cur_class_name.clone()
                } else {
                    spec.cur_ascend_class_name.clone()
                };
                if secondary != 0.0 {
                    let name = self.secondary_name(secondary);
                    if let Some(n) = name {
                        t = format!("{t} + {n}");
                    }
                }
                t
            },
        }
    }

    fn secondary_name(&self, id: f64) -> Option<String> {
        let tree = &self.result().env.spec.tree;
        match tree.json.alternate_ascendancies.as_ref() {
            Some(serde_json::Value::Array(a)) => a
                .get((id as usize).wrapping_sub(1))
                .and_then(|x| x["name"].as_str())
                .map(str::to_owned),
            _ => None,
        }
    }
}

/// One `controls.classDrop.list` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassEntry {
    pub label: String,
    pub class_id: usize,
    /// `ascendancies[i + 1] = { label, ascendClassId = i }`.
    pub ascendancies: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassSelect {
    /// Sorted by label.
    pub classes: Vec<ClassEntry>,
    /// `spec.curClassId` / `curAscendClassId`.
    pub class_id: Option<usize>,
    pub ascend_class_id: Option<usize>,
    /// `secondaryAscendDrop.list`: `("None", 0)` + the alternate ascendancies sorted by label.
    pub secondary_list: Vec<(String, f64)>,
    pub secondary_ascend_class_id: f64,
    /// The `(classText)` of `SetWindowTitleWithBuildClass` (PassiveSpec.lua:2390-2396).
    pub title_class: String,
}
