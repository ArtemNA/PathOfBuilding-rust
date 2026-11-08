//! CalcPerform.lua:3143-3316 -- extra curses (:3143-3192), ally curses (:3193-3211), the curse limit (:3215-3217) and the curse slots
//! (:3219-3316). R6.7, skills on only.
//!
//! * `pairs({[curses] = modDB, [minionCurses] = env.minion and env.minion.modDB})` (:3144): table keys -- a per-process order (R13); each
//!   entry writes its own `dest` and its own DB and only reads `enemyDB`, so the order cannot matter: Rust takes the player first.
//! * Ally curses (:3217-3238, R-party): [`ally_curses`] from the party tab's `Curse.Curse` -- slotted after the player's and the minion's.
//! * `minionCurses` (limit 1, or a spectre's `EnemyCurseLimit` + 1) gets the curses of the minions' own skills (:2909-2920) and the minion's
//!   extra curses (R7.10).
//! * The slots: `curseSlots` fills from 1 without holes (a slot is either the first empty one or an occupied one), so `#curseSlots` is the
//!   vector length. No `table.sort` (L17): the order is `activeSkillList` order, then the extra curses.

use std::rc::Rc;

use pob_data::skill_stat_map::{ModElem, SkillMod};
use pob_mods::{Mod, ModCfg, ModType};

use super::buffs::{Curse, determine_curse_priority, new_list, no_spaces, scale_add};
use super::{base, flag, inc, more, nm, set_cond, skill_list_of};
use crate::calc_tools::{GemLevelTarget, SkillInstance, build_skill_instance_stats, validate_gem_level};
use crate::env::Env;
use crate::output::Output;
use crate::setup::{list_mods, ok};
use crate::skill_modlist::{for_each_stat_mod, level_value};

/// CalcActiveSkill.lua:53-79 `calcs.mergeSkillInstanceMods(env, gemModList, { grantedEffect, level, quality = 0 })` for an ExtraCurse
/// (:3150-3154). The stat walk is `skill_modlist::for_each_stat_mod`; a malformed data mod in a curse effect
/// is not handled (none in the data reaches this call on the goldens) and stops with its line.
fn merge_extra_curse_mods(env: &Env, ge_idx: usize, level: Option<f64>) -> Vec<Rc<Mod>> {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.skills;
    let ge = &sk.effects[ge_idx];
    let level = validate_gem_level(
        &GemLevelTarget {
            granted_effect: ge,
            natural_max_level: None,
        },
        level,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let stats = build_skill_instance_stats(
        &SkillInstance {
            level,
            quality: 0.0,
            actor_level: None,
        },
        ge,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let mut out: Vec<Rc<Mod>> = Vec::new();
    let push = |m: &SkillMod, v: f64| match m {
        SkillMod::Mod(md) => {
            // `mergeLevelMod`: `copyTable(mod, true)` with the new value.
            let mut nmod = md.clone();
            nmod.value = level_value(&md.value, v);
            out.push(Rc::new(nmod));
        }
        SkillMod::Malformed(mm) => unimplemented!(
            "Modules/CalcActiveSkill.lua:31 malformed data mod {} in ExtraCurse {} (none reaches here on the goldens)",
            mm.name,
            ge.id
        ),
    };
    for_each_stat_mod(sk, ge, &stats, push).unwrap_or_else(|e| panic!("{e}"));
    if let Some(base_mods) = &ge.base_mods {
        for elem in base_mods {
            match elem {
                ModElem::Mod(SkillMod::Mod(m)) => out.push(Rc::new(m.clone())),
                ModElem::Mod(SkillMod::Malformed(m)) => unimplemented!(
                    "Modules/CalcActiveSkill.lua:78 malformed baseMod {} in ExtraCurse {} (none reaches here on the goldens)",
                    m.name,
                    ge.id
                ),
                ModElem::Group { .. } => unimplemented!("Modules/CalcActiveSkill.lua:78 AddList of a mod group in {}.baseMods", ge.id),
            }
        }
    }
    out
}

/// :3143-3192 extra curses: `curses` with `env.modDB`, then `minionCurses` with `env.minion.modDB` (see the module doc).
pub(super) fn extra_curses(env: &Env, curses: &mut Vec<Curse>, minion_curses: &mut Vec<Curse>) {
    extra_curses_for(env, &env.mod_db, curses);
    if let Some(m) = &env.minion {
        let mdb = m.borrow().mod_db.clone();
        extra_curses_for(env, &mdb, minion_curses);
    }
}

/// One `pairs` entry of :3144 (`dest`, `modDB`).
fn extra_curses_for(env: &Env, mod_db: &pob_mods::StoreRef, curses: &mut Vec<Curse>) {
    let mod_db = mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    if env.mode_combat && flag(&mod_db, &["Condition:OnConsecratedGround"]) {
        let effect = 1.0 + inc(&mod_db, &["ConsecratedGroundEffect"]) / 100.0;
        nm(
            &mod_db,
            "CurseEffectOnSelf",
            ModType::Inc,
            -50.0 * effect,
            "Consecrated Ground",
            0,
            0,
            vec![],
        );
    }
    let skills = env.skills.as_ref().expect("skills on").skills.clone();
    // Taken first: as the head of the `for` the borrow would last the whole loop, and a curse on the player writes to this store.
    let extra_curses = ok(mod_db.borrow().list(None, &["ExtraCurse"]));
    for value in extra_curses {
        let skill_id = value.get("skillId");
        // `env.data.skills[value.skillId]` -- a nil/unknown id is a nil grantedEffect: skipped.
        let Some(ge_idx) = skill_id.as_str().and_then(|id| skills.index.get(id).copied()) else {
            continue;
        };
        let gem_mod_list = merge_extra_curse_mods(env, ge_idx, value.get("level").as_num());
        let ge_name = skills.effects[ge_idx].name.clone();
        let curse_mod_list: Vec<Rc<Mod>> = gem_mod_list
            .iter()
            .filter(|m| {
                m.tags
                    .iter()
                    .any(|t| t.get("type").as_str() == Some("GlobalEffect") && t.get("effectType").as_str() == Some("Curse"))
            })
            .cloned()
            .collect();
        if value.get("applyToPlayer").truthy() {
            // :3167-3177 curses on the player respect no limit.
            if base(&mod_db, &["AvoidCurse"]) < 100.0 {
                set_cond(&mod_db, "Cursed", true);
                super::bump_mult(&mod_db, "CurseOnSelf", 1.0);
                set_cond(&mod_db, &format!("AffectedBy{}", no_spaces(&ge_name)), true);
                let cfg = ModCfg {
                    skill_name: Some(Rc::from(ge_name.as_str())),
                    ..ModCfg::default()
                };
                let gml = new_list();
                gml.borrow_mut().add_list(&gem_mod_list);
                let inc_ = super::sum(&mod_db, ModType::Inc, Some(&cfg), &["CurseEffectOnSelf"])
                    + super::sum(&gml, ModType::Inc, None, &["CurseEffectAgainstPlayer"]);
                let more_ = more(&mod_db, Some(&cfg), &["CurseEffectOnSelf"]) * more(&gml, None, &["CurseEffectAgainstPlayer"]);
                ok(mod_db
                    .borrow_mut()
                    .scale_add_list(&curse_mod_list, pob_lua::number::max2((1.0 + inc_ / 100.0) * more_, 0.0), false));
            }
        } else if !flag(&enemy_db, &["Hexproof"]) || flag(&mod_db, &["CursesIgnoreHexproof"]) {
            let priority = determine_curse_priority(env, &ge_name, None);
            let l = new_list();
            scale_add(
                &l,
                &curse_mod_list,
                (1.0 + inc(&enemy_db, &["CurseEffectOnSelf"]) / 100.0) * more(&enemy_db, None, &["CurseEffectOnSelf"]),
            );
            curses.push(Curse {
                name: ge_name,
                priority,
                is_mark: false,
                ignore_hex_limit: false,
                socketed_curses_hex_limit: false,
                mod_list: Some(list_mods(&l)),
                buff_mod_list: None,
                minion_buff_mod_list: None,
            });
        }
    }
}

/// `allyCurses` (:3217-3238, R-party): the party members' curses (`partyMembers.Curse.Curse`), priority 0, scaled by their effect (a
/// non-mark also by the enemy's `CurseEffectOnSelf` MORE), and `allyCurses.limit` (the tab's "Curse Limit" line, nil without curses).
pub(super) struct AllyCurses {
    pub curses: Vec<Curse>,
    pub limit: pob_mods::Value,
}

pub(super) fn ally_curses(env: &Env) -> AllyCurses {
    let party = env.party.clone();
    let mut out = AllyCurses {
        curses: Vec::new(),
        limit: pob_mods::Value::Nil,
    };
    // `if allyPartyCurses["Curse"] then allyCurses.limit = allyPartyCurses.limit else allyPartyCurses = { Curse = {} } end`
    let Some(g) = party.curse.group("Curse") else {
        return out;
    };
    out.limit = party.curse.limit.clone();
    for (curse_name, curse) in super::buffs::party_entries(g, 3230) {
        let mut mult = super::buffs::effect_mult_num(&curse.effect_mult, 3230) / 100.0;
        if !curse.is_mark {
            mult *= more(&env.enemy_db, None, &["CurseEffectOnSelf"]);
        }
        let l = new_list();
        scale_add(&l, &curse.mod_list, mult);
        out.curses.push(Curse {
            name: curse_name.clone(),
            priority: 0.0,
            is_mark: curse.is_mark,
            ignore_hex_limit: false,
            socketed_curses_hex_limit: false,
            mod_list: Some(list_mods(&l)),
            buff_mod_list: None,
            minion_buff_mod_list: None,
        });
    }
    out
}

/// :3241-3341 the curse limit and the slots; returns `curseSlots`.
pub(super) fn curse_slots(
    env: &Env,
    output: &mut Output,
    curses: &[Curse],
    minion_curses: &[Curse],
    minion_limit: f64,
    ally: &AllyCurses,
) -> Vec<Rc<Curse>> {
    let mod_db = env.mod_db.clone();
    // :3215 `Flag and output.PowerChargesMax or Sum(...)`
    let limit = if flag(&mod_db, &["CurseLimitIsMaximumPowerCharges"])
        && let Some(v) = output.opt_num("PowerChargesMax")
    {
        v
    } else {
        base(&mod_db, &["EnemyCurseLimit"])
    };
    output.set_num("EnemyCurseLimit", limit);
    let st = env.skills.as_ref().expect("skills on");
    let main_cfg = super::player_main_skill(env, 3232).cfg;
    let curses: Vec<Rc<Curse>> = curses.iter().cloned().map(Rc::new).collect();
    let minion_curses: Vec<Rc<Curse>> = minion_curses.iter().cloned().map(Rc::new).collect();
    let mut slots: Vec<Rc<Curse>> = Vec::new();
    // Currently assume only 1 mark is possible
    let mut mark_slotted = false;
    // :3250 `ipairs({curses, minionCurses, allyCurses})`
    let player_limit = limit;
    let ally_curses: Vec<Rc<Curse>> = ally.curses.iter().cloned().map(Rc::new).collect();
    let ally_limit = || match &ally.limit {
        pob_mods::Value::Num(n) => *n,
        _ => panic!("Modules/CalcPerform.lua:3265: 'for' limit must be a number"),
    };
    for (curse, limit) in curses
        .iter()
        .map(|c| (c, player_limit))
        .chain(minion_curses.iter().map(|c| (c, minion_limit)))
        .chain(ally_curses.iter().map(|c| (c, ally_limit())))
    {
        // Calculate curses that ignore hex limit after
        if curse.ignore_hex_limit || curse.socketed_curses_hex_limit {
            continue;
        }
        let mut slot: Option<usize> = None;
        let mut skip_adding_curse = false;
        // :3230-3239 check if we need to disable a certain curse aura.
        for (i, sk) in st.active_skill_list.iter().enumerate() {
            let first = sk.buff_list.first().and_then(|b| b.name.as_str());
            if first == Some(curse.name.as_str()) && sk.skill_types.contains(&super::skill_type_u32("Aura")) {
                let skill_flag = || {
                    super::skill_guard(&sk.malformed_mods, &["SelfAurasAffectYouAndLinkedTarget"]);
                    ok(skill_list_of(env, i)
                        .borrow()
                        .flag(Some(&main_cfg), &["SelfAurasAffectYouAndLinkedTarget"]))
                        == Some(true)
                };
                if flag(&mod_db, &["SelfAurasOnlyAffectYou"]) || skill_flag() {
                    skip_adding_curse = true;
                }
                break;
            }
        }
        // `for i = 1, source.limit do`
        let mut i = 1.0;
        while i <= limit {
            let idx = i as usize - 1;
            // Prevent multiple marks from being considered
            if curse.is_mark && mark_slotted {
                slot = None;
                break;
            }
            match slots.get(idx) {
                None => {
                    slot = Some(idx);
                    break;
                }
                Some(s) if s.name == curse.name => {
                    slot = if s.priority < curse.priority { Some(idx) } else { None };
                    break;
                }
                Some(s) if s.priority < curse.priority => slot = Some(idx),
                Some(_) => {}
            }
            i += 1.0;
        }
        if let Some(idx) = slot {
            if slots.get(idx).is_some_and(|s| s.is_mark) {
                mark_slotted = false;
            }
            if !skip_adding_curse {
                if idx == slots.len() {
                    slots.push(curse.clone());
                } else {
                    slots[idx] = curse.clone();
                }
            }
            if curse.is_mark {
                mark_slotted = true;
            }
        }
    }
    // :3280-3315 curses that ignore the hex limit (`ipairs({curses, minionCurses})`).
    for curse in curses.iter().chain(minion_curses.iter()) {
        if curse.ignore_hex_limit {
            let mut skip_adding_curse = false;
            for s in &mut slots {
                if s.name == curse.name {
                    // if curse is higher priority, replace current curse with it, otherwise if same or lower priority skip it entirely
                    if s.priority < curse.priority {
                        *s = curse.clone();
                    }
                    skip_adding_curse = true;
                    break;
                }
            }
            if !skip_adding_curse {
                slots.push(curse.clone());
            }
        }
        if curse.socketed_curses_hex_limit {
            let value = base(&mod_db, &["SocketedCursesHexLimitValue"]);
            let mut skip_adding_curse = false;
            for (i, s) in slots.iter_mut().enumerate() {
                if s.name == curse.name {
                    if s.priority < curse.priority {
                        *s = curse.clone();
                    }
                    skip_adding_curse = true;
                    break;
                }
                if (i + 1) as f64 >= value {
                    skip_adding_curse = true;
                }
            }
            if !skip_adding_curse {
                slots.push(curse.clone());
            }
        }
    }
    slots
}
