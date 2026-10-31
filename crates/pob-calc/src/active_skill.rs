//! Modules/CalcActiveSkill.lua:83-165 `calcs.createActiveSkill` (R6.3) and the tables it links: the skill effects (`activeEffect` /
//! `supportEffect`, CalcSetup.lua:1681-1693 / :1758-1768) and `groupCfg`/`gemCfg` (CalcSetup.lua:1612-1615, :1697-1699).
//!
//! Effects are Lua tables shared by reference between `supportLists`, every skill's `supportList`/`effectList` and the gem instances'
//! `displayEffect`/`supportEffect` (`addBestSupport` writes `superseded` onto the shared table): [`EffectRef`] = `Rc<RefCell<_>>`
//! compared by identity. Granted effects are indices into `pob_data::skills::Skills::effects`.
//!
//! The fields `buildActiveSkillModList` (CalcActiveSkill.lua:229-877, R6.4) writes are filled by [`crate::skill_modlist`]: `skillCfg`,
//! `skillData`, `skillModList`, weapon flags/cfgs, the rest of `skillFlags`, `disableReason`, `buffList`, the minion stub, and on the
//! effects `grantedEffectLevel`/`actorLevel`/`supportQuality`.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::skill_stat_map::MalformedMod;
use pob_data::skills::{GrantedEffect, SkillTypeSet, Skills};
use pob_mods::{Mod, ModCfg, StoreRef, Str, TabEntry, Value};

use crate::calc_tools::{ActiveSkillView, LuaError, WeaponView, can_granted_effect_support_active_skill_ext};

/// `groupCfg` (CalcSetup.lua:1612-1615): `slotName`, `propertyModList` and the gem colour counts `applySocketMods`' callers add (:1706-1708).
#[derive(Clone, Debug, Default)]
pub struct GroupCfg {
    pub slot_name: Option<String>,
    /// `env.modDB:Tabulate("LIST", {slotName = slotName}, "GemProperty")` -- one table, shared by every `copyTable(groupCfg, true)`.
    pub property_mod_list: Rc<Vec<TabEntry>>,
    pub intelligence_gems: Option<f64>,
    pub dexterity_gems: Option<f64>,
    pub strength_gems: Option<f64>,
}

impl GroupCfg {
    /// The table as a ModStore `cfg` (only the fields ModStore reads).
    pub fn mod_cfg(&self) -> ModCfg {
        ModCfg {
            slot_name: self.slot_name.as_deref().map(Rc::from),
            intelligence_gems: self.intelligence_gems,
            dexterity_gems: self.dexterity_gems,
            strength_gems: self.strength_gems,
            ..ModCfg::default()
        }
    }
}

/// `effect.gemCfg = copyTable(groupCfg, true)` + `socketColor`/`socketNum` (CalcSetup.lua:1697-1699) -- a snapshot of `groupCfg`.
#[derive(Clone, Debug, Default)]
pub struct GemCfg {
    pub group: GroupCfg,
    pub socket_color: Option<String>,
    pub socket_num: f64,
}

impl GemCfg {
    pub fn mod_cfg(&self) -> ModCfg {
        ModCfg {
            socket_color: self.socket_color.as_deref().map(Rc::from),
            socket_num: Some(self.socket_num),
            ..self.group.mod_cfg()
        }
    }
}

/// A skill effect table (`activeEffect`, `supportEffect`, an extra-support effect, the default Melee effect). `None` = the Lua field is nil.
#[derive(Clone, Debug, Default)]
pub struct SkillEffect {
    /// `effect.grantedEffect` -- index into `Skills::effects`.
    pub granted_effect: usize,
    pub level: Option<f64>,
    pub quality: Option<f64>,
    pub global_quality: Option<f64>,
    pub item_quality: Option<f64>,
    pub support_quality: Option<f64>,
    pub socket_quality: Option<f64>,
    /// `effect.srcInstance` -- (group, gem) indices into `socketGroupList` (0-based).
    pub src_instance: Option<(usize, usize)>,
    /// `effect.gemData` -- a `data.gems` id.
    pub gem_data: Option<String>,
    pub superseded: Option<bool>,
    /// `effect.isSupporting` -- a set keyed by gem instances; insertion order kept.
    pub is_supporting: Option<Vec<(usize, usize)>>,
    pub gem_cfg: Option<GemCfg>,
    pub applies_to_granted_skills: Option<bool>,
    pub enabled: Option<bool>,
    /// `effect.req` (`applyGemMods` key "req").
    pub req: Option<f64>,
    /// `effect.gemPropertyInfo` -- the `GemProperty` entries that matched.
    pub gem_property_info: Option<Vec<TabEntry>>,
    /// Any other `applyGemMods` key (`effect[value.key]`), in insertion order.
    pub other: IndexMap<String, f64>,
    /// `effect.grantedEffectLevel = grantedEffect.levels[effect.level]` (CalcActiveSkill.lua:514/:579) -- the level key.
    pub granted_effect_level: Option<f64>,
    /// `effect.actorLevel` (CalcActiveSkill.lua:577) -- nil for the player's skills.
    pub actor_level: Option<f64>,
}

/// A shared effect table.
#[derive(Clone)]
pub struct EffectRef(pub Rc<RefCell<SkillEffect>>);

impl EffectRef {
    pub fn new(e: SkillEffect) -> EffectRef {
        EffectRef(Rc::new(RefCell::new(e)))
    }

    pub fn borrow(&self) -> Ref<'_, SkillEffect> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, SkillEffect> {
        self.0.borrow_mut()
    }

    pub fn ptr_eq(&self, o: &EffectRef) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}

impl PartialEq for EffectRef {
    fn eq(&self, o: &Self) -> bool {
        self.ptr_eq(o)
    }
}

impl std::fmt::Debug for EffectRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let e = self.0.borrow();
        write!(f, "Effect#{}@{:?}", e.granted_effect, e.level)
    }
}

/// `env.player.activeSkillList[i]`: what `createActiveSkill` (CalcSetup) and `buildActiveSkillModList` (R6.4) build.
#[derive(Clone)]
pub struct ActiveSkill {
    pub active_effect: EffectRef,
    pub support_list: Vec<EffectRef>,
    /// `activeSkill.summonSkill` -- nil for the player's skills; for a minion skill (`createMinionSkills`, R7.10) the index of the summoning
    /// skill in `env.player.activeSkillList`.
    pub summon_skill: Option<usize>,
    /// `activeSkill.socketGroup` -- index into `socketGroupList`; nil for the default Melee.
    pub socket_group: Option<usize>,
    pub skill_types: SkillTypeSet,
    pub minion_skill_types: Option<SkillTypeSet>,
    /// `activeSkill.skillFlags` after `createActiveSkill` (`baseFlags` copy, `hit`, the supports' `addFlags`), insertion order.
    pub skill_flags: IndexMap<String, bool>,
    pub effect_list: Vec<EffectRef>,
    /// `activeSkill.slotName` (CalcSetup.lua:1838, gems only).
    pub slot_name: Option<String>,
    // --- buildActiveSkillModList (R6.4, crate::skill_modlist)
    pub skill_cfg: Option<SkillCfg>,
    pub weapon1_cfg: Option<SkillCfg>,
    pub weapon2_cfg: Option<SkillCfg>,
    pub weapon1_flags: Option<u32>,
    pub weapon2_flags: Option<u32>,
    /// `activeSkill.skillData`, insertion order.
    pub skill_data: IndexMap<String, Value>,
    /// `activeSkill.skillModList` (= `baseSkillModList`, the same table) -- a ModList whose parent is `actor.modDB`.
    pub skill_mod_list: Option<StoreRef>,
    /// Malformed mods (`SkillMod::Malformed`) the Lua list would hold; every query that reaches their name raises `query_error()`.
    pub malformed_mods: Vec<MalformedMod>,
    pub buff_list: Vec<Buff>,
    pub extra_skill_mod_list: Option<Vec<Rc<Mod>>>,
    pub minion_list: Option<Vec<String>>,
    pub minion: Option<Box<crate::minion::Minion>>,
    pub triggered_by: Option<EffectRef>,
    pub skill_part: Option<f64>,
    pub skill_part_name: Option<String>,
    pub skill_totem_id: Option<f64>,
    pub disable_reason: Option<String>,
    pub active_mine_count: Option<f64>,
    pub active_stage_count: Option<f64>,
    /// `activeSkill.infoMessage` / `infoMessage2` / `infoTrigger` -- written by `calcs.triggers` (CalcTriggers.lua, R7.8); `infoTrigger` is
    /// the `trigger` of a `calcFullDPS` entry (Calcs.lua:179/:254), `infoMessage*` are read by `calcs.mirages` (R7.9) and the UI.
    pub info_message: Option<String>,
    pub info_message2: Option<String>,
    pub info_trigger: Option<String>,
}

impl std::fmt::Debug for ActiveSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActiveSkill({:?}, flags {:?})", self.active_effect, self.skill_flags)
    }
}

impl ActiveSkill {
    /// `skillFlags[k]` truthy.
    pub fn flag(&self, k: &str) -> bool {
        self.skill_flags.get(k).copied().unwrap_or(false)
    }
}

/// `activeSkill.skillCfg` / `weapon1Cfg` / `weapon2Cfg` (CalcActiveSkill.lua:464-489). `skillTypes` is the skill's own table (shared, read
/// from [`ActiveSkill::skill_types`]); a weapon cfg's `skillCond` reads through to the skill cfg's (`__index`).
#[derive(Clone, Debug, Default)]
pub struct SkillCfg {
    pub flags: f64,
    pub keyword_flags: f64,
    pub skill_name: String,
    pub summon_skill_name: Option<String>,
    /// `skillGem` -- a `data.gems` id.
    pub skill_gem: Option<String>,
    /// `skillGrantedEffect` -- an effect index.
    pub skill_granted_effect: usize,
    pub skill_part: Option<f64>,
    /// The cfg's own `skillCond` table.
    pub skill_cond: IndexMap<String, bool>,
    /// `true` for a weapon cfg (`setmetatable(..., { __index = skillCfg.skillCond })`).
    pub skill_cond_inherits: bool,
    /// `env.mode_effective and effectiveRange`.
    pub skill_dist: Option<f64>,
    pub slot_name: Option<String>,
    pub socket_color: Option<String>,
    pub socket_num: Option<f64>,
}

impl SkillCfg {
    /// The table as a ModStore `cfg`. `parent_cond` = the skill cfg's `skillCond` (for a weapon cfg's `__index`).
    pub fn mod_cfg(&self, skills: &Skills, skill_types: &SkillTypeSet, parent_cond: Option<&IndexMap<String, bool>>) -> ModCfg {
        let ge = &skills.effects[self.skill_granted_effect];
        let mut cond: IndexMap<Str, bool> = IndexMap::new();
        if self.skill_cond_inherits
            && let Some(p) = parent_cond
        {
            for (k, v) in p {
                cond.insert(Str::from(k.as_str()), *v);
            }
        }
        for (k, v) in &self.skill_cond {
            cond.insert(Str::from(k.as_str()), *v);
        }
        ModCfg {
            flags: Some(self.flags),
            keyword_flags: Some(self.keyword_flags),
            skill_name: Some(Str::from(self.skill_name.as_str())),
            summon_skill_name: self.summon_skill_name.as_deref().map(Str::from),
            skill_dist: self.skill_dist,
            skill_cond: Some(cond),
            slot_name: self.slot_name.as_deref().map(Str::from),
            skill_gem: self
                .skill_gem
                .as_deref()
                .map(|g| crate::skills::skill_gem_cfg(&pob_data::gems::gems().gems[g])),
            socket_color: self.socket_color.as_deref().map(Str::from),
            socket_num: self.socket_num,
            skill_granted_effect: Some(pob_mods::GrantedEffect {
                id: Str::from(ge.id.as_str()),
                base_flags: ge
                    .base_flags
                    .as_ref()
                    .map(|f| f.iter().map(|k| (Str::from(k.as_str()), true)).collect()),
            }),
            skill_part: self.skill_part,
            skill_types: Some(skill_types.iter().map(|&t| f64::from(t)).collect()),
            ..ModCfg::default()
        }
    }
}

/// One `activeSkill.buffList` entry (CalcActiveSkill.lua:827-847). The fields copied off the `GlobalEffect` tag stay Lua values.
#[derive(Clone, Debug, Default)]
pub struct Buff {
    pub ty: Value,
    pub name: Value,
    pub allow_totem_buff: Value,
    pub cond: Value,
    pub enemy_cond: Value,
    pub stack_var: Value,
    pub stack_limit: Value,
    pub stack_limit_var: Value,
    pub apply_not_player: Value,
    pub apply_minions: Value,
    pub apply_allies: Value,
    pub active_skill_buff: Option<bool>,
    pub mod_list: Vec<Rc<Mod>>,
}

/// What `createActiveSkill` reads of the actor and the gem instances.
pub struct ActorView<'a> {
    pub weapon1: WeaponView<'a>,
    pub weapon2: WeaponView<'a>,
    /// `actor.enemy.player == actor`.
    pub is_player: bool,
}

/// The `fromItem` of a granted effect including CalcSetup's data write (`grantedEffect.fromItem = true`, CalcSetup.lua:1633-1634).
pub trait FromItem {
    fn is_from_item(&self, ge: &GrantedEffect) -> bool;
}

fn skill_type(name: &str) -> u32 {
    *pob_data::globals::globals()
        .skill_type
        .get(name)
        .unwrap_or_else(|| panic!("SkillType.{name} missing")) as u32
}

/// CalcActiveSkill.lua:83-165 `calcs.createActiveSkill(activeEffect, supportList, actor, socketGroup, summonSkill)` with `summonSkill` nil.
/// `src_from_item` = `activeEffect.srcInstance and activeEffect.srcInstance.fromItem`.
pub fn create_active_skill(
    skills: &Skills,
    active_effect: EffectRef,
    support_list: Vec<EffectRef>,
    actor: &ActorView<'_>,
    socket_group: Option<usize>,
    src_from_item: bool,
    from_item: &dyn FromItem,
) -> Result<ActiveSkill, LuaError> {
    create_active_skill_summoned(
        skills,
        active_effect,
        support_list,
        actor,
        socket_group,
        src_from_item,
        from_item,
        None,
    )
}

/// The summoning skill of a minion skill as `createActiveSkill` gets it (`summonSkill`, R7.10): its index in `env.player.activeSkillList`
/// and what `canGrantedEffectSupportActiveSkill` reads of it.
pub struct Summon<'a> {
    pub index: usize,
    pub view: crate::calc_tools::SummonView<'a>,
}

/// [`create_active_skill`] with `summonSkill` (CalcActiveSkill.lua:83-165): the supports' `addFlags` are not applied to a minion skill (:153).
#[allow(clippy::too_many_arguments)]
pub fn create_active_skill_summoned(
    skills: &Skills,
    active_effect: EffectRef,
    support_list: Vec<EffectRef>,
    actor: &ActorView<'_>,
    socket_group: Option<usize>,
    src_from_item: bool,
    from_item: &dyn FromItem,
    summon: Option<Summon<'_>>,
) -> Result<ActiveSkill, LuaError> {
    let (act_idx, has_gem_data, src_instance) = {
        let e = active_effect.borrow();
        (e.granted_effect, e.gem_data.is_some(), e.src_instance)
    };
    let act = &skills.effects[act_idx];
    // :97-101 `copyTable(activeGrantedEffect.skillTypes)` (R4: a new set)
    let mut skill_types = act
        .skill_types
        .clone()
        .ok_or_else(|| "Modules/Common.lua:479: bad argument #1 to 'pairs' (table expected, got nil)".to_owned())?;
    let minion_skill_types = act.minion_skill_types.clone();
    // :103-107 `copyTable(activeGrantedEffect.baseFlags)`
    let mut skill_flags: IndexMap<String, bool> = act
        .base_flags
        .as_ref()
        .ok_or_else(|| "Modules/Common.lua:479: bad argument #1 to 'pairs' (table expected, got nil)".to_owned())?
        .iter()
        .map(|f| (f.clone(), true))
        .collect();
    if !skill_flags.get("hit").copied().unwrap_or(false)
        && (skill_types.contains(&skill_type("Attack"))
            || skill_types.contains(&skill_type("Damage"))
            || skill_types.contains(&skill_type("Projectile")))
    {
        skill_flags.insert("hit".into(), true);
    }

    let can = |ge: &GrantedEffect, applies: bool, types: &SkillTypeSet| -> Result<bool, LuaError> {
        let view = ActiveSkillView {
            granted_effect: act,
            has_gem_data,
            src_instance_from_item: src_from_item,
            skill_types: Some(types),
            minion_skill_types: minion_skill_types.as_ref(),
            summon: summon.as_ref().map(|s| s.view),
            actor_is_player: actor.is_player,
            weapon1: actor.weapon1,
            weapon2: actor.weapon2,
        };
        can_granted_effect_support_active_skill_ext(ge, &view, false, applies, from_item.is_from_item(ge), from_item.is_from_item(act))
    };
    let add_types = |ge: &GrantedEffect, types: &mut SkillTypeSet| -> Result<(), LuaError> {
        let list = ge
            .add_skill_types
            .as_ref()
            .ok_or_else(|| "Modules/CalcActiveSkill.lua:117: bad argument #1 to 'pairs' (table expected, got nil)".to_owned())?;
        for &t in list {
            types.insert(t as u32);
        }
        Ok(())
    };

    // :110-123 Pass 1: add skill types from compatible supports. The `else` belongs to `if grantedEffect.support`, so the "rejected"
    // indices are the NON-support effects.
    let mut rejected: Vec<Option<usize>> = Vec::new();
    for (index, se) in support_list.iter().enumerate() {
        let (ge_idx, applies) = {
            let e = se.borrow();
            (e.granted_effect, e.applies_to_granted_skills.unwrap_or(false))
        };
        let ge = &skills.effects[ge_idx];
        if ge.support {
            if can(ge, applies, &skill_types)? {
                add_types(ge, &mut skill_types)?;
            }
        } else {
            rejected.push(Some(index));
        }
    }
    // :125-141 loop over the rejected until none is added; `rejectedSupportsIndices[index] = nil` + `ipairs` stops at the first hole.
    loop {
        let mut not_added_new_support = true;
        let mut i = 0;
        while i < rejected.len() {
            let Some(idx) = rejected[i] else { break };
            let (ge_idx, applies) = {
                let e = support_list[idx].borrow();
                (e.granted_effect, e.applies_to_granted_skills.unwrap_or(false))
            };
            let ge = &skills.effects[ge_idx];
            if ge.support && can(ge, applies, &skill_types)? {
                not_added_new_support = false;
                rejected[i] = None;
                add_types(ge, &mut skill_types)?;
            }
            i += 1;
        }
        if not_added_new_support {
            break;
        }
    }

    // :143-160 Pass 2: add all compatible supports.
    let mut effect_list = vec![active_effect.clone()];
    for se in &support_list {
        let (ge_idx, applies) = {
            let e = se.borrow();
            (e.granted_effect, e.applies_to_granted_skills.unwrap_or(false))
        };
        let ge = &skills.effects[ge_idx];
        if ge.support && can(ge, applies, &skill_types)? {
            effect_list.push(se.clone());
            if let Some(src) = src_instance {
                let mut e = se.borrow_mut();
                if let Some(set) = e.is_supporting.as_mut()
                    && !set.contains(&src)
                {
                    set.push(src);
                }
            }
            if let Some(flags) = ge.add_flags.as_ref().filter(|_| summon.is_none()) {
                for (k, _) in flags.iter() {
                    skill_flags.insert(k.clone(), true);
                }
            }
        }
    }

    Ok(ActiveSkill {
        active_effect,
        support_list,
        summon_skill: summon.as_ref().map(|s| s.index),
        socket_group,
        skill_types,
        minion_skill_types,
        skill_flags,
        effect_list,
        slot_name: None,
        skill_cfg: None,
        weapon1_cfg: None,
        weapon2_cfg: None,
        weapon1_flags: None,
        weapon2_flags: None,
        skill_data: IndexMap::new(),
        skill_mod_list: None,
        malformed_mods: Vec::new(),
        buff_list: Vec::new(),
        extra_skill_mod_list: None,
        minion_list: None,
        minion: None,
        triggered_by: None,
        skill_part: None,
        skill_part_name: None,
        skill_totem_id: None,
        disable_reason: None,
        active_mine_count: None,
        active_stage_count: None,
        info_message: None,
        info_message2: None,
        info_trigger: None,
    })
}
