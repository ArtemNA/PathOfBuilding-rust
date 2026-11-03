//! The gem model of Classes/SkillsTab.lua (R6.2): `Load` (:523-569) -> `LoadSkill` (:441-522) -> `ProcessSocketGroup` (:1161-1236) for
//! every group, then `SetActiveSkillSet` (:1478-1497) with `RebuildImbuedSupportBySlot` (:1464-1475); plus `FindSkillGem` (:1103-1127)
//! and `ProcessGemLevel` (:1129-1159). The result is `build.skillsTab` BEFORE CalcSetup appends the item/tree-granted groups.
//! Input: the raw XML attributes of `pob_build::SkillsModel`; data: `pob_data::{gems, skills}`.
//!
//! String-keyed `pairs` (MINEFIELD-RUST R13 -- LuaJIT's order differs per process): the variant loop of `LoadSkill` (:472-477, old save
//! format) and `FindSkillGem`'s walk over `data.gems` use the DATA order (`gems.json`, base variant before Alt). Gate:
//! `crates/pob-data/tests/gemdata.rs` (Rust result in the set of LuaJIT results over 20 processes).
//!
//! Not ported (no calc state): the UI controls, undo, `imbuedSupport`'s `gemChangeFunc` call in `LoadSkill` (:456-459 -- it passes
//! `gemMatch = nil`, i.e. CLEARS `imbuedSupportBySlot[slot]`; `SetActiveSkillSet` rebuilds the whole table right after, :1494).

use indexmap::IndexMap;
use pob_build::{GemModel, SkillsModel, SocketGroupModel, sanitise_text};
use pob_data::gems::{GemEntry, gems};
use pob_data::skills::{GrantedEffect, Skills};
use pob_items::Item;
use pob_lua::{Repl, gsub, match_};
use std::rc::Rc;

use crate::active_skill::EffectRef;
use crate::env::{GemEdit, GemOverride, Slot};

use crate::calc_tools::{GemLevelTarget, LuaError, get_gem_stat_requirement, validate_gem_level};

/// One `socketGroup.gemList[i]` (`gemInstance`). `None` = the Lua field is nil.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GemInstance {
    pub name_spec: Option<String>,
    pub gem_id: Option<String>,
    pub skill_id: Option<String>,
    pub level: Option<f64>,
    pub quality: Option<f64>,
    pub enabled: bool,
    /// nil on an explode gem CalcSetup creates (CalcSetup.lua:1535-1542).
    pub enable_global1: Option<bool>,
    /// nil on a gem CalcSetup creates for a granted skill (CalcSetup.lua:1486-1491).
    pub enable_global2: Option<bool>,
    /// nil on a gem CalcSetup creates.
    pub count: Option<f64>,
    pub skill_part: Option<f64>,
    pub skill_part_calcs: Option<f64>,
    pub skill_stage_count: Option<f64>,
    pub skill_stage_count_calcs: Option<f64>,
    pub skill_mine_count: Option<f64>,
    pub skill_mine_count_calcs: Option<f64>,
    pub skill_minion: Option<String>,
    pub skill_minion_calcs: Option<String>,
    pub skill_minion_item_set: Option<f64>,
    pub skill_minion_item_set_calcs: Option<f64>,
    pub skill_minion_skill: Option<f64>,
    pub skill_minion_skill_calcs: Option<f64>,
    // --- ProcessSocketGroup
    pub color: Option<String>,
    pub err_msg: Option<String>,
    /// `gemInstance.gemData` (a `data.gems` id).
    pub gem_data: Option<String>,
    /// `gemInstance.grantedEffect` (a `data.skills` id; only for skills without a gem).
    pub granted_effect: Option<String>,
    pub req_level: Option<f64>,
    pub req_str: Option<f64>,
    pub req_dex: Option<f64>,
    pub req_int: Option<f64>,
    pub natural_max_level: Option<f64>,
    pub new: Option<bool>,
    /// Set by CalcSetup for granted triggered skills; never by the load.
    pub triggered: bool,
    // --- set by `UpdateSocketGroups` (SkillsTab.lua:1237-1272) and CalcSetup (R6.3)
    pub matches_socket: Option<bool>,
    pub from_item: Option<bool>,
    pub no_supports: Option<bool>,
    pub trigger_chance: Option<f64>,
    pub explode_source: Option<ExplodeSource>,
    /// `gemInstance.displayEffect` / `.supportEffect` (MAIN mode, CalcSetup.lua:1672-1673, :1693-1694, :1786).
    pub display_effect: Option<EffectRef>,
    pub support_effect: Option<EffectRef>,
    /// `gemInstance.reqOverride = effect.req` (CalcSetup.lua:1702, :1776).
    pub req_override: Option<f64>,
}

/// A Lua table reference compared by identity (`==` on tables).
#[derive(Debug)]
pub struct ByRef<T>(pub Rc<T>);

impl<T> Clone for ByRef<T> {
    fn clone(&self) -> Self {
        ByRef(self.0.clone())
    }
}

impl<T> PartialEq for ByRef<T> {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}

/// One `env.explodeSources` entry: a tree node (`buildModListForNode`'s second value) or an item (CalcSetup.lua:837-839).
#[derive(Clone, Debug, PartialEq)]
pub enum ExplodeSource {
    Node(u32),
    Item(ByRef<Item>),
}

impl ExplodeSource {
    /// `explodeSource.modSource or explodeSource.id` (CalcSetup.lua:1526) -- a tree node has no `modSource`.
    pub fn key(&self) -> String {
        match self {
            ExplodeSource::Node(id) => format!("node:{id}"),
            ExplodeSource::Item(i) => format!("item:{}", i.0.mod_source.as_deref().unwrap_or("")),
        }
    }
}

/// One `socketGroup`.
#[derive(Clone, Debug, PartialEq)]
pub struct SocketGroup {
    pub enabled: bool,
    /// nil when the attribute is absent.
    pub include_in_full_dps: Option<bool>,
    pub group_count: Option<f64>,
    pub label: Option<String>,
    pub slot: Option<String>,
    pub source: Option<String>,
    /// nil on a group CalcSetup creates, until it is the main group (CalcSetup.lua:1883).
    pub main_active_skill: Option<f64>,
    pub main_active_skill_calcs: Option<f64>,
    pub imbued_support: Option<String>,
    pub gem_list: Vec<GemInstance>,
    // --- set by CalcSetup (R6.3)
    pub source_item: Option<ByRef<Item>>,
    pub source_node: Option<u32>,
    pub no_supports: Option<bool>,
    pub explode_sources: Option<Vec<ExplodeSource>>,
    pub slot_enabled: Option<bool>,
    pub display_label: Option<String>,
    /// `group.displaySkillList` -- indices into `env.player.activeSkillList` (0-based).
    pub display_skill_list: Option<Vec<usize>>,
    /// `group.displayGemList` -- (group, gem) indices into `socketGroupList` (0-based).
    pub display_gem_list: Option<Vec<(usize, usize)>>,
}

/// `skillSets[id]`.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillSet {
    pub id: f64,
    pub title: Option<String>,
    pub socket_group_list: Vec<SocketGroup>,
}

/// `build.skillsTab` after `Load`.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillsTab {
    pub default_gem_level: String,
    pub default_gem_quality: f64,
    /// In `skillSetOrderList` order.
    pub skill_sets: Vec<SkillSet>,
    pub active_skill_set_id: f64,
    /// `imbuedSupportBySlot[slot] = grantedEffect` (effect id), in insertion order.
    pub imbued_support_by_slot: IndexMap<String, String>,
}

impl SocketGroup {
    /// `{ }` -- every field nil / false.
    pub fn new_empty() -> SocketGroup {
        SocketGroup {
            enabled: false,
            include_in_full_dps: None,
            group_count: None,
            label: None,
            slot: None,
            source: None,
            main_active_skill: None,
            main_active_skill_calcs: None,
            imbued_support: None,
            gem_list: Vec::new(),
            source_item: None,
            source_node: None,
            no_supports: None,
            explode_sources: None,
            slot_enabled: None,
            display_label: None,
            display_skill_list: None,
            display_gem_list: None,
        }
    }
}

impl SkillsTab {
    /// `self.socketGroupList` (the active set's list).
    pub fn socket_group_list(&self) -> &[SocketGroup] {
        &self.active_set().socket_group_list
    }

    pub fn socket_group_list_mut(&mut self) -> &mut Vec<SocketGroup> {
        let id = self.active_skill_set_id;
        &mut self
            .skill_sets
            .iter_mut()
            .find(|s| s.id == id)
            .expect("active skill set")
            .socket_group_list
    }

    fn active_set(&self) -> &SkillSet {
        self.skill_sets
            .iter()
            .find(|s| s.id == self.active_skill_set_id)
            .expect("active skill set")
    }
}

fn color_code(name: &str) -> String {
    pob_data::globals::globals()
        .color_codes
        .get(name)
        .cloned()
        .unwrap_or_else(|| panic!("colorCodes.{name} missing"))
}

/// SkillsTab.lua:441-522 `LoadSkill` for one `<Skill>` (the `ProcessSocketGroup` at its end included).
pub fn load_skill(group: &SocketGroupModel, skills: &Skills) -> Result<SocketGroup, LuaError> {
    let data = gems();
    let mut gem_list = Vec::with_capacity(group.gems.len());
    for g in &group.gems {
        gem_list.push(load_gem(g, skills, data));
    }
    let mut sg = SocketGroup {
        enabled: group.enabled,
        include_in_full_dps: group.include_in_full_dps_attr.then_some(group.include_in_full_dps),
        group_count: group.group_count,
        label: group.label.clone(),
        slot: group.slot.clone(),
        source: group.source.clone(),
        main_active_skill: Some(group.main_active_skill),
        main_active_skill_calcs: Some(group.main_active_skill_calcs),
        // :456 `if node.attrib.imbuedSupport and node.attrib.slot` (pob-build keeps it only then)
        imbued_support: group.imbued_support.clone(),
        gem_list,
        ..SocketGroup::new_empty()
    };
    process_socket_group(&mut sg, skills)?;
    Ok(sg)
}

/// :462-512, one `<Gem>`.
fn load_gem(g: &GemModel, skills: &Skills, data: &pob_data::gems::Gems) -> GemInstance {
    let mut gi = GemInstance {
        // :463 `sanitiseText(child.attrib.nameSpec or "")` -- done by pob-build
        name_spec: Some(g.name_spec.clone()),
        ..GemInstance::default()
    };
    if let Some(game_id) = &g.gem_id {
        let mut gem_data: Option<&GemEntry> = None;
        if let Some(variants) = data.gems_by_game_id.get(game_id) {
            if let Some(v) = &g.variant_id {
                gem_data = variants.get(v).map(|id| &data.gems[id]);
            } else if let Some(skill_id) = &g.skill_id {
                // R13: `pairs(possibleVariants)` -- data order (base variant first)
                gem_data = variants
                    .values()
                    .map(|id| &data.gems[id])
                    .find(|v| &v.granted_effect_id == skill_id);
            }
        }
        if let Some(gd) = gem_data {
            gi.gem_id = Some(gd.id.clone());
            gi.skill_id = Some(gd.granted_effect_id.clone());
            // :481 `gemInstance.nameSpec = gemData.nameSpec` -- gems have no `nameSpec`: nil
            gi.name_spec = None;
        }
    } else if let Some(skill_id) = &g.skill_id
        && let Some(ge) = skills.get(skill_id)
    {
        // :486 `data.gemForSkill[grantedEffect]` -- keyed by the effect TABLE (D4), the converter re-keyed it by id
        gi.gem_id = data.gem_for_skill.get(&ge.id).cloned();
        gi.skill_id = Some(ge.id.clone());
        gi.name_spec = Some(ge.name.clone());
    }
    gi.level = g.level;
    gi.quality = g.quality;
    gi.name_spec = gi.name_spec.as_deref().map(sanitise_text);
    gi.enabled = g.enabled;
    gi.enable_global1 = Some(g.enable_global1);
    gi.enable_global2 = Some(g.enable_global2);
    gi.count = Some(g.count);
    gi.skill_part = g.skill_part;
    gi.skill_part_calcs = g.skill_part_calcs;
    gi.skill_stage_count = g.skill_stage_count;
    gi.skill_stage_count_calcs = g.skill_stage_count_calcs;
    gi.skill_mine_count = g.skill_mine_count;
    gi.skill_mine_count_calcs = g.skill_mine_count_calcs;
    gi.skill_minion = g.skill_minion.clone();
    gi.skill_minion_calcs = g.skill_minion_calcs.clone();
    gi.skill_minion_item_set = g.skill_minion_item_set;
    gi.skill_minion_item_set_calcs = g.skill_minion_item_set_calcs;
    gi.skill_minion_skill = g.skill_minion_skill;
    gi.skill_minion_skill_calcs = g.skill_minion_skill_calcs;
    gi
}

/// `nameSpec:match("%S")`.
fn has_non_space(s: &str) -> bool {
    s.bytes().any(|b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
}

/// The write of [`crate::env::CalcOverride::gem_edit`] on a loaded tab. No `ProcessSocketGroup` after it, as upstream.
pub fn apply_gem_override(tab: &mut SkillsTab, ov: &GemOverride) -> Result<(), LuaError> {
    let group = tab
        .socket_group_list_mut()
        .iter_mut()
        .filter(|g| g.source.is_none())
        .nth(ov.group)
        .ok_or_else(|| format!("CalcOverride.gem_edit: no socket group {}", ov.group))?;
    let gi = group.gem_list.get_mut(ov.gem);
    match (&ov.edit, gi) {
        (GemEdit::Candidate { gem_data, level, .. }, Some(gi)) => {
            gi.level = Some(*level);
            gi.gem_data = Some(gem_data.clone());
            gi.display_effect = None;
        }
        // GemSelectControl.lua:82-92
        (
            GemEdit::Candidate {
                gem_data,
                level,
                default_quality,
            },
            None,
        ) => {
            let gd = &gems().gems[gem_data];
            group.gem_list.push(GemInstance {
                level: Some(*level),
                quality: Some(*default_quality),
                count: Some(1.0),
                enabled: true,
                enable_global1: Some(true),
                enable_global2: Some(true),
                gem_id: Some(gem_data.clone()),
                name_spec: Some(gd.name.clone()),
                skill_id: Some(gd.granted_effect_id.clone()),
                gem_data: Some(gem_data.clone()),
                ..GemInstance::default()
            });
        }
        (GemEdit::Quality(q), Some(gi)) => gi.quality = Some(*q),
        (GemEdit::ToggleEnabled, Some(gi)) => gi.enabled = !gi.enabled,
        (_, None) => return Err(format!("CalcOverride.gem_edit: group {} has no gem {}", ov.group, ov.gem)),
    }
    Ok(())
}

/// SkillsTab.lua:1161-1236 `ProcessSocketGroup`.
pub fn process_socket_group(sg: &mut SocketGroup, skills: &Skills) -> Result<(), LuaError> {
    process_socket_group_ext(sg, skills, &mut Vec::new())
}

/// `ProcessSocketGroup` recording the data mutation of :1186-1190 (`grantedEffect.levels[level].cost = {}` for a triggered granted
/// skill) in `cost_wiped` as `(effect id, level)` -- the data here is immutable; whoever reads `cost` (R6.4+) must consult the list.
pub fn process_socket_group_ext(sg: &mut SocketGroup, skills: &Skills, cost_wiped: &mut Vec<(String, f64)>) -> Result<(), LuaError> {
    let data = gems();
    for gi in &mut sg.gem_list {
        gi.color = Some("^8".to_owned());
        if gi.name_spec.is_none() {
            gi.name_spec = Some(String::new());
        }
        let prev_default_level = match gi.gem_data.as_ref().map(|id| &data.gems[id]) {
            Some(gd) => Some(gd.natural_max_level),
            None => (gi.new == Some(true)).then_some(20.0),
        };
        gi.gem_data = None;
        gi.granted_effect = None;
        if let Some(gem_id) = gi.gem_id.clone() {
            gi.err_msg = None;
            if let Some(gd) = data.gems.get(&gem_id) {
                gi.gem_data = Some(gem_id);
                gi.name_spec = Some(gd.name.clone());
                gi.skill_id = Some(gd.granted_effect_id.clone());
            }
        } else if let Some(skill_id) = gi.skill_id.clone() {
            gi.err_msg = None;
            // :1180 `data.gemForSkill[gemInstance.skillId]` indexes the table-keyed map with a STRING (D4): always nil
            gi.granted_effect = skills.get(&skill_id).map(|ge| ge.id.clone());
            if gi.triggered {
                // :1186-1190 `gemInstance.grantedEffect.levels[gemInstance.level].cost = {}`
                let ge = skills
                    .get(&skill_id)
                    .ok_or_else(|| "Classes/SkillsTab.lua:1187: attempt to index field 'grantedEffect' (a nil value)".to_owned())?;
                if let Some(l) = gi.level.and_then(|l| ge.level(l)) {
                    let k = (ge.id.clone(), l.level);
                    if !cost_wiped.contains(&k) {
                        cost_wiped.push(k);
                    }
                }
            }
        } else if has_non_space(gi.name_spec.as_deref().unwrap_or("")) {
            let (err, gd) = find_skill_gem(gi.name_spec.as_deref().unwrap_or(""))?;
            gi.err_msg = err;
            gi.gem_id = gd.map(|g| g.id.clone());
            gi.skill_id = gd.map(|g| g.granted_effect_id.clone());
            if let Some(g) = gd {
                gi.gem_data = Some(g.id.clone());
                gi.name_spec = Some(g.name.clone());
            }
        } else {
            gi.err_msg = None;
            gi.gem_data = None;
            gi.skill_id = None;
        }
        // :1204 `gemData.grantedEffect.unsupported` -- nil for every effect of the data
        let ge: Option<&GrantedEffect> = match (&gi.granted_effect, &gi.gem_data) {
            (Some(id), _) => skills.get(id),
            (None, Some(g)) => skills.get(&data.gems[g].granted_effect_id),
            (None, None) => None,
        };
        let Some(ge) = ge else { continue };
        gi.new = None;
        gi.color = Some(match ge.color {
            Some(1.0) => color_code("STRENGTH"),
            Some(2.0) => color_code("DEXTERITY"),
            Some(3.0) => color_code("INTELLIGENCE"),
            _ => color_code("NORMAL"),
        });
        let gem_data = gi.gem_data.as_ref().map(|id| &data.gems[id]);
        if let (Some(prev), Some(gd)) = (prev_default_level, gem_data)
            && gd.natural_max_level != prev
        {
            gi.level = Some(gd.natural_max_level);
            gi.natural_max_level = gi.level;
        }
        let target = GemLevelTarget {
            granted_effect: ge,
            natural_max_level: gem_data.map(|g| g.natural_max_level),
        };
        gi.level = Some(validate_gem_level(&target, gi.level)?);
        if let Some(gd) = gem_data {
            let level = gi.level.expect("validated");
            let req = ge.level(level).and_then(|l| l.level_requirement);
            gi.req_level = req;
            let stat = |multi: f64| -> Result<f64, LuaError> {
                match req {
                    Some(r) => Ok(get_gem_stat_requirement(r, ge.support, multi)),
                    None if multi == 0.0 => Ok(0.0),
                    None => Err("Modules/CalcTools.lua:191: attempt to perform arithmetic on local 'level' (a nil value)".into()),
                }
            };
            gi.req_str = Some(stat(gd.req_str)?);
            gi.req_dex = Some(stat(gd.req_dex)?);
            gi.req_int = Some(stat(gd.req_int)?);
        }
    }
    Ok(())
}

fn ascii_class_repl(f: impl Fn(u8) -> Vec<u8>) -> impl FnMut(&[u8], &pob_lua::Captures) -> Option<Vec<u8>> {
    move |s: &[u8], caps: &pob_lua::Captures| {
        let m = caps.get(0).and_then(|c| c.bytes(s)).expect("the whole match");
        Some(f(m[0]))
    }
}

fn pe(e: pob_lua::PatternError) -> LuaError {
    format!("Classes/SkillsTab.lua:1105: {e:?}")
}

/// SkillsTab.lua:1103-1127 `FindSkillGem(nameSpec)` -> `(errMsg, gemData)`. Walks `data.gems` in data order (R13): only the text of an
/// "Ambiguous" error depends on it (which two of the matches it names).
pub fn find_skill_gem(name_spec: &str) -> Result<(Option<String>, Option<&'static GemEntry>), LuaError> {
    let spec = name_spec.as_bytes();
    let nospace = gsub(spec, b" ", Repl::Str(b""), None).map_err(pe)?.0;
    let upper_lower = |a: u8| vec![b'[', a.to_ascii_uppercase(), a.to_ascii_lowercase(), b']'];
    let mut f1 = ascii_class_repl(upper_lower);
    let mut f5 = ascii_class_repl(move |a| [b".*".as_slice(), &upper_lower(a)].concat());
    let cat = |parts: &[&[u8]]| parts.concat();
    let pattern_list: [Vec<u8>; 5] = [
        cat(&[b"^ ", &gsub(spec, b"%a", Repl::Fn(&mut f1), None).map_err(pe)?.0, b"$"]),
        cat(&[b"^", &gsub(spec, b"%a", Repl::Str(b" %0%%l+"), None).map_err(pe)?.0, b"$"]),
        cat(&[b"^ ", &gsub(&nospace, b"%l", Repl::Str(b"%%l*%0"), None).map_err(pe)?.0, b"%l+$"]),
        cat(&[b"^", &gsub(&nospace, b"%a", Repl::Str(b".*%0"), None).map_err(pe)?.0]),
        cat(&[b"^", &gsub(&nospace, b"%a", Repl::Fn(&mut f5), None).map_err(pe)?.0]),
    ];
    let data = gems();
    for pattern in &pattern_list {
        let mut found: Option<&'static GemEntry> = None;
        for gem in data.gems.values() {
            let subject = [b" ".as_slice(), gem.name.as_bytes()].concat();
            if match_(&subject, pattern, None).map_err(pe)?.is_some() {
                if let Some(f) = found {
                    return Ok((
                        Some(format!("Ambiguous gem name '{name_spec}': matches '{}', '{}'", f.name, gem.name)),
                        None,
                    ));
                }
                found = Some(gem);
            }
        }
        if found.is_some() {
            return Ok((None, found));
        }
    }
    Ok((Some(format!("Unrecognised gem name '{name_spec}'")), None))
}

/// SkillsTab.lua:1129-1159 `ProcessGemLevel(gemData, imbued)` with `self.defaultGemLevel` and `self.build.characterLevel`.
pub fn process_gem_level(
    gem: &GemEntry,
    imbued: bool,
    default_gem_level: &str,
    character_level: Option<f64>,
    skills: &Skills,
) -> Result<f64, LuaError> {
    let ge = skills.get(&gem.granted_effect_id).expect("gem's granted effect");
    let nml = gem.natural_max_level;
    Ok(match default_gem_level {
        _ if imbued => 1.0,
        "levelOne" => 1.0,
        "awakenedMaximum" => nml + 1.0,
        "corruptedMaximum" => {
            if ge.plus_version_of.is_some() {
                nml
            } else {
                nml + 1.0
            }
        }
        "normalMaximum" => nml,
        _ => {
            let mut max_gem_level = nml;
            if ge.level(max_gem_level).is_none() {
                max_gem_level = ge.levels_len as f64;
            }
            let character_level = character_level.unwrap_or(1.0);
            let mut gem_level = max_gem_level;
            while gem_level >= 1.0 {
                let lv = ge
                    .level(gem_level)
                    .ok_or_else(|| "Classes/SkillsTab.lua:1152: attempt to index a nil value".to_owned())?;
                let req = lv
                    .level_requirement
                    .ok_or_else(|| "Classes/SkillsTab.lua:1152: attempt to compare nil with number".to_owned())?;
                if req <= character_level {
                    return Ok(gem_level);
                }
                gem_level -= 1.0;
            }
            1.0
        }
    })
}

/// SkillsTab.lua:1464-1475 `RebuildImbuedSupportBySlot` over the active list.
fn rebuild_imbued_support_by_slot(tab: &mut SkillsTab) {
    let data = gems();
    let mut out = IndexMap::new();
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

/// SkillsTab.lua:523-569 `Load` (the skill-set bookkeeping of pob-build's `SkillsModel`) + `SetActiveSkillSet` (:1478-1497).
pub fn load_skills_tab(model: &SkillsModel, skills: &Skills) -> Result<SkillsTab, LuaError> {
    let mut skill_sets = Vec::with_capacity(model.sets.len());
    for set in &model.sets {
        let mut list = Vec::with_capacity(set.groups.len());
        for g in &set.groups {
            list.push(load_skill(g, skills)?);
        }
        skill_sets.push(SkillSet {
            id: set.id,
            title: set.title.clone(),
            socket_group_list: list,
        });
    }
    let mut tab = SkillsTab {
        default_gem_level: model.default_gem_level.clone(),
        default_gem_quality: model.default_gem_quality,
        skill_sets,
        active_skill_set_id: model.active_skill_set,
        imbued_support_by_slot: IndexMap::new(),
    };
    rebuild_imbued_support_by_slot(&mut tab);
    Ok(tab)
}

/// SkillsTab.lua:1237-1272 `UpdateSocketGroups` over the active list -- Build.lua:691 runs it right before the first `BuildOutput`, so it
/// sees only the groups of the XML (the granted ones CalcSetup appends later keep no `matchesSocket`).
pub fn update_socket_groups(tab: &mut SkillsTab, slots: &[Slot], items: &IndexMap<u64, Rc<Item>>, skills: &Skills) {
    let data = gems();
    let mut slot_socketed_counts: IndexMap<String, usize> = IndexMap::new();
    for sg in tab.socket_group_list_mut() {
        // Clear stale matches when a group is no longer assigned to an item.
        for gi in &mut sg.gem_list {
            gi.matches_socket = Some(false);
        }
        let Some(slot_name) = sg.slot.clone() else { continue };
        let gem_offset = slot_socketed_counts.get(&slot_name).copied().unwrap_or(0);
        for (i, gi) in sg.gem_list.iter_mut().enumerate() {
            let ge = match (&gi.granted_effect, &gi.gem_data) {
                (Some(id), _) => skills.get(id),
                (None, Some(g)) => skills.get(&data.gems[g].granted_effect_id),
                (None, None) => None,
            };
            let Some(ge) = ge else { continue };
            let gem_idx = gem_offset + i + 1;
            // `self.build.itemsTab.slots[socketGroup.slot]`
            let Some(slot) = slots.iter().find(|s| s.slot_name == slot_name) else {
                continue;
            };
            let item = (slot.sel_item_id >= 0.0 && slot.sel_item_id.fract() == 0.0)
                .then(|| items.get(&(slot.sel_item_id as u64)))
                .flatten();
            // `item.sockets` is always a table on a parsed item
            if let Some(item) = item {
                if item.sockets_colour_always_matches {
                    gi.matches_socket = Some(true);
                } else {
                    let colour = match ge.color {
                        Some(1.0) => Some("R"),
                        Some(2.0) => Some("G"),
                        Some(3.0) => Some("B"),
                        _ => None,
                    };
                    // `item.sockets[gemIdx] and (item.sockets[gemIdx].color == gemColour)` -- nil when there is no such socket
                    gi.matches_socket = item.sockets.get(gem_idx - 1).map(|s| Some(s.color.as_str()) == colour);
                }
            }
        }
        slot_socketed_counts.insert(slot_name, gem_offset + sg.gem_list.len());
    }
}
