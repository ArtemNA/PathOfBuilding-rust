//! Modules/CalcActiveSkill.lua:83-165 `calcs.createActiveSkill` (R6.3) and the tables it links: the skill effects (`activeEffect` /
//! `supportEffect`, CalcSetup.lua:1681-1693 / :1758-1768) and `groupCfg`/`gemCfg` (CalcSetup.lua:1612-1615, :1697-1699).
//!
//! Effects are Lua tables shared by reference between `supportLists`, every skill's `supportList`/`effectList` and the gem instances'
//! `displayEffect`/`supportEffect` (`addBestSupport` writes `superseded` onto the shared table): [`EffectRef`] = `Rc<RefCell<_>>`
//! compared by identity. Granted effects are indices into `pob_data::skills::Skills::effects`.
//!
//! Not here (R6.4, `buildActiveSkillModList` CalcActiveSkill.lua:229-877): `skillCfg`, `skillData`, `skillModList`, weapon flags, the rest of
//! `skillFlags`, `disableReason`, the `srcInstance.skillPart*`/`skillMinion*`/... writes, `activeEffect.grantedEffectLevel`/`supportQuality`.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::skills::{GrantedEffect, SkillTypeSet, Skills};
use pob_mods::{ModCfg, TabEntry};

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

/// `env.player.activeSkillList[i]` as far as CalcSetup builds it.
#[derive(Clone, Debug)]
pub struct ActiveSkill {
    pub active_effect: EffectRef,
    pub support_list: Vec<EffectRef>,
    /// `activeSkill.summonSkill` -- always nil for the player's skills (minions are R7).
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
            summon: None,
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
            if let Some(flags) = &ge.add_flags {
                for (k, _) in flags.iter() {
                    skill_flags.insert(k.clone(), true);
                }
            }
        }
    }

    Ok(ActiveSkill {
        active_effect,
        support_list,
        summon_skill: None,
        socket_group,
        skill_types,
        minion_skill_types,
        skill_flags,
        effect_list,
        slot_name: None,
    })
}
