//! CalcPerform.lua:3317-3377 -- the guard slots (:3317-3341) and the application of buffs, debuffs and curse slots to the actors
//! (:3343-3377). R6.7, skills on only.
//!
//! Hash-order walks (MINEFIELD-RUST R13: the keys are buff names, LuaJIT's `pairs` order differs per process; Rust walks the insertion
//! order of [`Buffs`]):
//!
//! | place | what the order decides | class |
//! |---|---|---|
//! | `ipairs(guardOrder)` :3418 | which guards get a slot: `Vaal Molten Shell` wipes and stops, otherwise every `^Vaal` guard plus the FIRST non-Vaal one | a sequence: the order the guards were first merged (5606cc64f #10489; it was `pairs(guards)`, R13) -- the insertion order of [`Buffs`] |
//! | `ipairs(guardSlots)` :3334 | condition writes (idempotent) and `mergeBuff` into distinct `buffs[guard.name]` keys | independent |
//! | `pairs(buffs)` :3344 | the order of `modDB:AddList` (the order inside a ModDB bucket = the summation order) and `+1` on `BuffOnSelf` (integers) | a sum of 3+ float terms of one bucket may differ in the last bit (L10) -- the same freedom LuaJIT has between its own runs |
//! | `pairs(debuffs)` :3358 | the order of `enemyDB:AddList` | as `pairs(buffs)` |
//! | `pairs(minionBuffs)` :3354 | `AddList` into the minion's DB: a multiset (R7.10) | insertion order |
//! | `ipairs(curseSlots)` :3362 | a sequence | -- |

use std::rc::Rc;

use super::buffs::{Buffs, Curse, merge_buff, no_spaces};
use super::{bump_mult, cond, set_cond, set_mult};
use crate::env::Env;
use crate::output::Output;

/// :3317-3341 the guard slots -> `buffs[guard.name]`; `GuardSkillActive`.
pub(super) fn guard_slots(env: &mut Env, guards: &Buffs, buffs: &mut Buffs, output: &mut Output) {
    let mod_db = env.mod_db.clone();
    let mut slots: Vec<(&str, &[Rc<pob_mods::Mod>])> = Vec::new();
    let mut non_vaal = false;
    // :3418-3419 `for _, name in ipairs(guardOrder)`: the names in the order they first went into `guards` (:2455-2457)
    for (name, list) in guards {
        if name == "Vaal Molten Shell" {
            slots.clear();
            non_vaal = false;
            slots.push((name, &list.mods));
            break;
        } else if name.starts_with("Vaal") {
            slots.push((name, &list.mods));
        } else {
            if !non_vaal {
                slots.push((name, &list.mods));
                non_vaal = true;
            }
        }
    }
    if non_vaal {
        set_cond(&mod_db, "AffectedByNonVaalGuardSkill", true);
    }
    for (name, list) in slots {
        set_cond(&mod_db, "AffectedByGuardSkill", true);
        set_cond(&mod_db, &format!("AffectedBy{}", no_spaces(name)), true);
        merge_buff(list, buffs, name);
    }
    output.set_bool("GuardSkillActive", cond(&mod_db, "AffectedByGuardSkill").unwrap_or(false));
}

/// :3343-3376 apply the buffs/debuffs and the curse slots.
pub(super) fn apply(env: &Env, buffs: &Buffs, minion_buffs: &Buffs, debuffs: &Buffs, curse_slots: &[Rc<Curse>]) {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let minion = env
        .minion
        .as_ref()
        .map(|m| (m.borrow().mod_db.clone(), super::env_minion(env).ty.clone()));
    let main = env.minion.as_ref().map(|_| super::player_main_skill(env, 3423));
    for list in buffs.values() {
        mod_db.borrow_mut().add_list(&list.mods);
        if !list.not_buff {
            bump_mult(&mod_db, "BuffOnSelf", 1.0);
        }
        // :3349-3351 `addMinionModifiers(modList, env.player.mainSkill.skillCfg, env.minion)`
        if let (Some((mdb, ty)), Some(main)) = (&minion, &main) {
            let l = pob_mods::ModStore::new_list(None);
            l.borrow_mut().add_list(&list.mods);
            crate::minion::add_minion_modifiers(&l, Some(&main.cfg), mdb, ty);
        }
    }
    // :3353-3357
    if let Some((mdb, _)) = &minion {
        for list in minion_buffs.values() {
            mdb.borrow_mut().add_list(&list.mods);
        }
    }
    for list in debuffs.values() {
        enemy_db.borrow_mut().add_list(&list.mods);
    }
    set_mult(&mod_db, "CurseOnEnemy", curse_slots.len() as f64);
    for slot in curse_slots {
        set_cond(&enemy_db, "Cursed", true);
        if slot.is_mark {
            set_cond(&enemy_db, "Marked", true);
        }
        if let Some(l) = &slot.mod_list {
            enemy_db.borrow_mut().add_list(l);
        }
        if let Some(l) = &slot.buff_mod_list {
            mod_db.borrow_mut().add_list(l);
        }
        if let Some(l) = &slot.minion_buff_mod_list {
            let (mdb, _) = minion
                .as_ref()
                .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3447: attempt to index field 'minion' (a nil value)"));
            mdb.borrow_mut().add_list(l);
        }
    }
}
