//! CalcPerform.lua:3317-3377 -- the guard slots (:3317-3341) and the application of buffs, debuffs and curse slots to the actors
//! (:3343-3377). R6.7, skills on only.
//!
//! Hash-order walks (MINEFIELD-RUST R13: the keys are buff names, LuaJIT's `pairs` order differs per process; Rust walks the insertion
//! order of [`Buffs`]):
//!
//! | place | what the order decides | class |
//! |---|---|---|
//! | `pairs(guards)` :3320 | which guards get a slot: `Vaal Molten Shell` wipes and stops, otherwise every `^Vaal` guard plus the FIRST non-Vaal one | **chooses** when there are 2+ non-Vaal guards (or Vaal Molten Shell after another) -- counted in `SkillsState::order_notes`; one guard on every golden |
//! | `ipairs(guardSlots)` :3334 | condition writes (idempotent) and `mergeBuff` into distinct `buffs[guard.name]` keys | independent |
//! | `pairs(buffs)` :3344 | the order of `modDB:AddList` (the order inside a ModDB bucket = the summation order) and `+1` on `BuffOnSelf` (integers) | a sum of 3+ float terms of one bucket may differ in the last bit (L10) -- the same freedom LuaJIT has between its own runs |
//! | `pairs(debuffs)` :3358 | the order of `enemyDB:AddList` | as `pairs(buffs)` |
//! | `pairs(minionBuffs)` :3354 | -- (`env.minion` is nil, R7) | -- |
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
    let mut non_vaal_seen = 0usize;
    for (name, list) in guards {
        if name == "Vaal Molten Shell" {
            slots.clear();
            non_vaal = false;
            slots.push((name, &list.mods));
            break;
        } else if name.starts_with("Vaal") {
            slots.push((name, &list.mods));
        } else {
            non_vaal_seen += 1;
            if !non_vaal {
                slots.push((name, &list.mods));
                non_vaal = true;
            }
        }
    }
    // The walk chooses (R13) when two non-Vaal guards compete, or when Vaal Molten Shell may come after other guards.
    if non_vaal_seen > 1 || (guards.contains_key("Vaal Molten Shell") && guards.len() > 1) {
        let names: Vec<&str> = guards.keys().map(String::as_str).collect();
        if let Some(st) = env.skills.as_mut() {
            st.order_notes.push(format!(
                "Modules/CalcPerform.lua:3320 pairs(guards) chooses the guard slots among {names:?} (LuaJIT: per-process order)"
            ));
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
pub(super) fn apply(env: &Env, buffs: &Buffs, debuffs: &Buffs, curse_slots: &[Rc<Curse>]) {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    for list in buffs.values() {
        mod_db.borrow_mut().add_list(&list.mods);
        if !list.not_buff {
            bump_mult(&mod_db, "BuffOnSelf", 1.0);
        }
        // :3349-3351 `addMinionModifiers` -- env.minion is nil (R7).
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
        // :3373-3375 `minionBuffModList` -- only made when env.minion is set (R7).
    }
}
