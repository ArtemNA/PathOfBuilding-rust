//! CalcPerform.lua:3667-3790 -- the Life pools of allies whose Life is spent before the player's (`takenFrom*BeforeYou`): minions
//! (Sentinel of Radiance, Void Spawn, Stone Golem, spectres, Companionship) and totems (the nearest Totem, Vaal Rejuvenation Totem) -- R6.8.
//!
//! * The pool tables are the file-level locals of :1255-1271. `allyLifeRedirects` (:1267-1271) appends the redirects of
//!   `pairs({ minionLifePoolBySkill, totemLifePoolByStat })` -- string keys inside, a per-process order in LuaJIT (MINEFIELD-RUST R13); it is
//!   only the name list of one `Sum`, so Rust takes the source order.
//! * A minion's Life (:3711-3728, R7.10): another skill's minion is built here (`initMinionModDB` / `addMinionModifiers` /
//!   `doActorAttribsConditions(env, minion)`), `env.minion` only gets `doActorLifeMana` again ([`minion_life_of`]).
//! * Totems are the player's own data: `calcs.calcTotemLife` (CalcOffence.lua:338-342) is ported here.

use indexmap::IndexMap;
use pob_build::ConfigValue;
use pob_lua::number::min2;
use pob_mods::ModType;

use std::cell::RefCell;
use std::rc::Rc;

use super::buffs::Buffs;
use super::{ActorState, base, nm, nm_flag, override_, skill_cfg_of, skill_list_of, skill_type_u32};
use crate::env::Env;

/// One pool of :1256-1266.
struct Pool {
    life: &'static str,
    redirect: &'static str,
    source: &'static str,
    limit: Option<&'static str>,
}

/// `minionLifePoolBySkill` (:1256-1260).
const MINION_LIFE_POOL_BY_SKILL: [(&str, Pool); 3] = [
    (
        "Summon Sentinel of Radiance",
        Pool {
            life: "TotalRadianceSentinelLife",
            redirect: "takenFromRadianceSentinelBeforeYou",
            source: "Sentinel of Radiance",
            limit: None,
        },
    ),
    (
        "Summon Void Spawn",
        Pool {
            life: "TotalVoidSpawnLife",
            redirect: "takenFromVoidSpawnBeforeYou",
            source: "Void Spawns",
            limit: Some("ActiveVoidSpawnLimit"),
        },
    ),
    (
        "Summon Stone Golem of Safeguarding",
        Pool {
            life: "TotalStoneGolemLife",
            redirect: "takenFromStoneGolemBeforeYou",
            source: "Stone Golem",
            limit: None,
        },
    ),
];

/// `spectreLifePool` (:1261).
const SPECTRE_LIFE_POOL: Pool = Pool {
    life: "TotalSpectreLife",
    redirect: "takenFromSpectresBeforeYou",
    source: "Spectres",
    limit: Some("ActiveSpectreLimit"),
};

/// `companionshipLifePool` (:1262).
const COMPANIONSHIP_LIFE_POOL: Pool = Pool {
    life: "TotalMinionLife",
    redirect: "takenFromMinionBeforeYou",
    source: "Minion",
    limit: None,
};

/// `totemLifePoolByStat.TotalTotemLife` / `.TotalVaalRejuvenationTotemLife` (:1263-1266): (redirect, source).
const TOTEM_POOL: (&str, &str) = ("takenFromTotemsBeforeYou", "Totem");
const VAAL_TOTEM_POOL: (&str, &str) = ("takenFromVaalRejuvenationTotemsBeforeYou", "Vaal Rejuvenation Totem");

/// `allyLifeRedirects` (:1267-1271).
const ALLY_LIFE_REDIRECTS: [&str; 7] = [
    SPECTRE_LIFE_POOL.redirect,
    COMPANIONSHIP_LIFE_POOL.redirect,
    MINION_LIFE_POOL_BY_SKILL[0].1.redirect,
    MINION_LIFE_POOL_BY_SKILL[1].1.redirect,
    MINION_LIFE_POOL_BY_SKILL[2].1.redirect,
    TOTEM_POOL.0,
    VAAL_TOTEM_POOL.0,
];

/// One `env.player.allyLifeList[pool][i]` entry (`{ name, life, count }`; totems have no `count`). Read by CalcDefence.lua:2546 (breakdown only).
#[derive(Clone, Debug)]
#[allow(dead_code)] // the breakdown tables of CalcDefence.lua:2544-2557 are not ported (R8: they write no output)
pub struct AllyLifeEntry {
    pub name: String,
    pub life: f64,
    pub count: Option<f64>,
}

/// `env.player.allyLifeList`.
pub type AllyLifeList = IndexMap<String, Vec<AllyLifeEntry>>;

/// CalcOffence.lua:338-342 `calcs.calcTotemLife(env, activeSkill)` (the life only; shared with offence, [`crate::offence::calc_totem_life`]).
fn calc_totem_life(env: &Env, i: usize, level: f64, totem_id: f64) -> f64 {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    let q = crate::offence::Q {
        list: skill_list_of(env, i),
        malformed: sk.malformed_mods.clone(),
    };
    crate::offence::calc_totem_life(&q, &skill_cfg_of(st, sk), level, Some(totem_id)).0
}

struct Totem {
    name: String,
    life: f64,
}

/// :3667-3790. `summoned_minion_override` is the local of :1504-1505 (`multiplierSummonedMinion`, nil when 0).
pub(super) fn ally_life(
    env: &mut Env,
    player: &mut ActorState,
    summoned_minion_override: Option<&ConfigValue>,
    skip_ehp: bool,
    buffs: Option<&(Buffs, Buffs)>,
    env_minion_state: Option<&Rc<RefCell<ActorState>>>,
    aegis_item: Option<Rc<pob_items::Item>>,
) {
    let mod_db = env.mod_db.clone();
    // :3668
    let needs_ally_life = !skip_ehp && base(&mod_db, &ALLY_LIFE_REDIRECTS) != 0.0;
    let mut list: AllyLifeList = IndexMap::new();
    if needs_ally_life {
        let skill_names: Vec<String> = match &env.skills {
            Some(st) => st
                .active_skill_list
                .iter()
                .map(|sk| st.skills.effects[sk.active_effect.borrow().granted_effect].name.clone())
                .collect(),
            None => Vec::new(),
        };
        // :3674 `m_min(output.ActiveSpectreLimit or 0, summonedMinionOverride or m_huge)` (evaluated before the loop).
        let override_limit = match summoned_minion_override {
            None => f64::INFINITY,
            Some(ConfigValue::Num(v)) => *v,
            Some(other) => panic!("Modules/CalcPerform.lua:3674: bad argument #2 to 'min' (number expected, got {other:?})"),
        };
        let spectre_limit = min2(player.output.opt_num("ActiveSpectreLimit").unwrap_or(0.0), override_limit);

        // :3675-3751 the minions.
        let undamagable = skill_type_u32("MinionsAreUndamagable");
        let n_skills = env.skills.as_ref().map_or(0, |st| st.active_skill_list.len());
        let mut calculated: Vec<&'static str> = Vec::new();
        let mut spectre_count = 0.0;
        // `firstSpectre`: (its minion's Life, the pool list key, the entry index)
        let mut first_spectre: Option<(f64, usize)> = None;
        for (i, skill_name) in skill_names.iter().enumerate().take(n_skills) {
            let (skip, name, pools) = {
                let st = env.skills.as_ref().expect("skills on");
                let sk = &st.active_skill_list[i];
                if sk.minion.is_none() || sk.flag("disable") || sk.skill_types.contains(&undamagable) {
                    (true, String::new(), Vec::new())
                } else {
                    let name = skill_name.clone();
                    let skill_pool = if sk.flag("spectre") {
                        Some(&SPECTRE_LIFE_POOL)
                    } else {
                        MINION_LIFE_POOL_BY_SKILL.iter().find(|(k, _)| *k == name).map(|(_, p)| p)
                    };
                    let mut pools: Vec<&'static Pool> = skill_pool.into_iter().collect();
                    // :3681-3693 Companionship is stored on the supported skill.
                    let has_companionship = sk
                        .buff_list
                        .iter()
                        .any(|b| b.mod_list.iter().any(|m| *m.name == *COMPANIONSHIP_LIFE_POOL.redirect));
                    if has_companionship {
                        pools.push(&COMPANIONSHIP_LIFE_POOL);
                        if let Some(sp) = skill_pool
                            && base(&mod_db, &[sp.redirect]) != 0.0
                            && base(&mod_db, &[COMPANIONSHIP_LIFE_POOL.redirect]) != 0.0
                        {
                            pools = vec![&COMPANIONSHIP_LIFE_POOL];
                            nm_flag(&mod_db, &format!("MinionLifeShares{}", sp.life), "Companionship");
                        }
                    }
                    (false, name, pools)
                }
            };
            if skip {
                continue;
            }
            let mut minion_life: Option<f64> = None;
            for pool in pools {
                let is_spectre = pool.life == SPECTRE_LIFE_POOL.life;
                let can_add = ((is_spectre && spectre_count < spectre_limit) || (!is_spectre && !calculated.contains(&pool.life)))
                    && base(&mod_db, &[pool.redirect]) != 0.0
                    && !override_(&mod_db, &[pool.life]).is_some_and(|v| v.truthy());
                if !can_add {
                    continue;
                }
                let life = match minion_life {
                    Some(l) => l,
                    None => {
                        let l = minion_life_of(env, i, buffs, env_minion_state, aegis_item.clone());
                        minion_life = Some(l);
                        l
                    }
                };
                // :3731-3736 Void Spawn redirects scale from their summon limit
                let mut count = match pool.limit {
                    Some(lim) => match player.output.opt_num(lim) {
                        Some(v) => v,
                        None => {
                            let st = env.skills.as_ref().expect("skills on");
                            let sk = &st.active_skill_list[i];
                            super::skill_guard(&sk.malformed_mods, &[lim]);
                            crate::setup::ok(crate::calc_lib::val(&skill_list_of(env, i).borrow(), lim, None))
                        }
                    },
                    None => 1.0,
                };
                if is_spectre {
                    count = 1.0;
                    spectre_count += 1.0;
                }
                let total = life * count;
                nm(&mod_db, pool.life, ModType::Base, total, pool.source, 0, 0, vec![]);
                let minion_name = {
                    let st = env.skills.as_ref().expect("skills on");
                    let n = st.active_skill_list[i].minion.as_ref().expect("minion").md("name");
                    n.as_str()
                        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3741: minionData.name is not a string"))
                        .to_owned()
                };
                let entries = list.entry(pool.life.to_owned()).or_default();
                entries.push(AllyLifeEntry {
                    name: minion_name,
                    life: total,
                    count: Some(count),
                });
                if is_spectre {
                    if first_spectre.is_none() {
                        first_spectre = Some((life, entries.len() - 1));
                    }
                } else {
                    calculated.push(pool.life);
                }
            }
            let _ = name;
        }
        // :3752-3758 the leftover spectre slots: the first Raise Spectre group fills them.
        if let Some((life, k)) = first_spectre
            && spectre_count < spectre_limit
        {
            let count = spectre_limit - spectre_count;
            let total = life * count;
            let e = &mut list.get_mut(SPECTRE_LIFE_POOL.life).expect("spectre pool")[k];
            e.count = Some(e.count.unwrap_or(0.0) + count);
            e.life += total;
            nm(
                &mod_db,
                SPECTRE_LIFE_POOL.life,
                ModType::Base,
                total,
                SPECTRE_LIFE_POOL.source,
                0,
                0,
                vec![],
            );
        }

        // :3760-3771 the totems.
        let mut normal_totem: Option<Totem> = None;
        let mut vaal_totem: Option<Totem> = None;
        let candidates: Vec<(usize, f64, f64)> = match &env.skills {
            Some(st) => st
                .active_skill_list
                .iter()
                .enumerate()
                .filter_map(|(i, sk)| {
                    let level = sk.skill_data.get("totemLevel").filter(|v| v.truthy())?;
                    if !sk.flag("totem") || sk.flag("disable") {
                        return None;
                    }
                    let totem_id = sk.skill_totem_id?;
                    // A non-number level indexes `monsterAllyLifeTable` to nil: the arithmetic error of `calc_totem_life`.
                    let level = level.as_num().unwrap_or(f64::NAN);
                    Some((i, level, totem_id))
                })
                .collect(),
            None => Vec::new(),
        };
        for (i, level, totem_id) in candidates {
            let life = calc_totem_life(env, i, level, totem_id);
            let totem = Totem {
                name: skill_names[i].clone(),
                life,
            };
            if totem.name == "Vaal Rejuvenation Totem" {
                vaal_totem = Some(totem);
            } else if normal_totem.as_ref().is_none_or(|t| life > t.life) {
                normal_totem = Some(totem);
            }
        }

        // :3773-3789 PoB cannot know which Totem is nearest, so use the eligible type with the most Life.
        let nearest_vaal = match (&vaal_totem, &normal_totem) {
            (Some(v), n) => n.as_ref().is_none_or(|n| v.life > n.life),
            (None, _) => false,
        };
        let nearest = if nearest_vaal { vaal_totem.as_ref() } else { normal_totem.as_ref() };
        let totem_life_override = override_(&mod_db, &["TotalTotemLife"]).is_some_and(|v| v.truthy());
        let totem_redirect = base(&mod_db, &[TOTEM_POOL.0]);
        let nearest_is_vaal = nearest.is_some() && nearest_vaal && !totem_life_override;
        if let Some(t) = nearest
            && !nearest_is_vaal
            && totem_redirect != 0.0
            && !totem_life_override
        {
            nm(&mod_db, "TotalTotemLife", ModType::Base, t.life, TOTEM_POOL.1, 0, 0, vec![]);
            list.insert(
                "TotalTotemLife".into(),
                vec![AllyLifeEntry {
                    name: t.name.clone(),
                    life: t.life,
                    count: None,
                }],
            );
        }
        if let Some(v) = &vaal_totem
            && (base(&mod_db, &[VAAL_TOTEM_POOL.0]) != 0.0 || nearest_is_vaal && totem_redirect != 0.0)
            && !override_(&mod_db, &["TotalVaalRejuvenationTotemLife"]).is_some_and(|x| x.truthy())
        {
            nm(
                &mod_db,
                "TotalVaalRejuvenationTotemLife",
                ModType::Base,
                v.life,
                VAAL_TOTEM_POOL.1,
                0,
                0,
                vec![],
            );
            list.insert(
                "TotalVaalRejuvenationTotemLife".into(),
                vec![AllyLifeEntry {
                    name: v.name.clone(),
                    life: v.life,
                    count: None,
                }],
            );
        }
    }
    player.ally_life_list = Some(list);
}

/// :3711-3728 `minion.output.Life` of `env.player.activeSkillList[i]`'s minion. Another skill's minion gets its DB here
/// (`initMinionModDB(env, activeSkill)` -- `minion.output` a fresh table --, its skill's and the buffs' `MinionModifier`s, the minion buffs,
/// its keystones) and its attributes (`doActorAttribsConditions`, through `env.minion` pointed at it for the call); `env.minion` only
/// its life and mana again (`doActorLifeMana`).
fn minion_life_of(
    env: &mut Env,
    i: usize,
    buffs: Option<&(Buffs, Buffs)>,
    env_minion_state: Option<&Rc<RefCell<ActorState>>>,
    aegis_item: Option<Rc<pob_items::Item>>,
) -> f64 {
    let (actor, ty) = {
        let st = env.skills.as_ref().expect("skills on");
        let m = st.active_skill_list[i].minion.as_deref().expect("activeSkill.minion");
        (m.actor.clone().expect("minion.modDB (:1297)"), m.ty.clone())
    };
    let mdb = actor.borrow().mod_db.clone();
    let is_env_minion = env.minion.as_ref().is_some_and(|a| Rc::ptr_eq(a, &actor));
    if is_env_minion {
        let ms = env_minion_state.expect("env.minion has its output");
        let mut ms = ms.borrow_mut();
        super::do_actor_life_mana(&mdb, &mut ms.output);
        return ms.output.num("Life");
    }
    let saved = env.minion.replace(actor.clone());
    let mut state = ActorState::new(&actor.borrow().actor);
    crate::minion::init_minion_mod_db(env, i, aegis_item);
    let (list, cfg, malformed) = {
        let st = env.skills.as_ref().expect("skills on");
        let sk = &st.active_skill_list[i];
        (skill_list_of(env, i), skill_cfg_of(st, sk), sk.malformed_mods.clone())
    };
    super::skill_guard(&malformed, &["MinionModifier"]);
    crate::minion::add_minion_modifiers(&list, Some(&cfg), &mdb, &ty);
    if let Some((b, mb)) = buffs {
        for l in b.values() {
            let tmp = pob_mods::ModStore::new_list(None);
            tmp.borrow_mut().add_list(&l.mods);
            crate::minion::add_minion_modifiers(&tmp, Some(&cfg), &mdb, &ty);
        }
        for l in mb.values() {
            mdb.borrow_mut().add_list(&l.mods);
        }
    }
    for name in super::list(&mdb, &["Keystone"]) {
        if let Some(mods) = name.as_str().and_then(|n| env.spec.tree.keystone_mods(n)) {
            mdb.borrow_mut().add_list(mods);
        }
    }
    super::do_actor_attribs_conditions(env, super::Who::Minion, &mut state);
    env.minion = saved;
    state.output.num("Life")
}
