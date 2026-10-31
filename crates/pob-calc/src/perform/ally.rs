//! CalcPerform.lua:3667-3790 -- the Life pools of allies whose Life is spent before the player's (`takenFrom*BeforeYou`): minions
//! (Sentinel of Radiance, Void Spawn, Stone Golem, spectres, Companionship) and totems (the nearest Totem, Vaal Rejuvenation Totem) -- R6.8.
//!
//! * The pool tables are the file-level locals of :1255-1271. `allyLifeRedirects` (:1267-1271) appends the redirects of
//!   `pairs({ minionLifePoolBySkill, totemLifePoolByStat })` -- string keys inside, a per-process order in LuaJIT (MINEFIELD-RUST R13); it is
//!   only the name list of one `Sum`, so Rust takes the source order.
//! * Minions as actors are R7: `initMinionModDB` / `addMinionModifiers` / `doActorAttribsConditions(env, minion)` / `doActorLifeMana(minion)`
//!   (:3712-3729) build the minion's Life. Where a pool needs it, the skill is recorded in `env.unimplemented` and adds nothing.
//! * Totems are the player's own data: `calcs.calcTotemLife` (CalcOffence.lua:338-342) is ported here.

use indexmap::IndexMap;
use pob_build::ConfigValue;
use pob_data::constants::{constants, monster_table_at};
use pob_lua::number::{min2, round};
use pob_lua::tostring_num;
use pob_mods::ModType;

use super::{ActorState, base, nm, nm_flag, override_, skill_cfg_of, skill_guard, skill_list_of, skill_type_u32};
use crate::calc_lib;
use crate::env::Env;
use crate::setup::ok;

/// One pool of :1256-1266.
struct Pool {
    life: &'static str,
    redirect: &'static str,
    #[allow(dead_code)] // read by the minion half (`pool.source`, :3738), which needs the minion's Life (R7)
    source: &'static str,
    #[allow(dead_code)] // read by the minion half (`pool.limit`, :3732), which needs the minion's Life (R7)
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

/// CalcOffence.lua:338-342 `calcs.calcTotemLife(env, activeSkill)` (the life only).
fn calc_totem_life(env: &Env, i: usize, level: f64, totem_id: f64) -> f64 {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    skill_guard(&sk.malformed_mods, &["TotemLife"]);
    let list = skill_list_of(env, i);
    let life_mod = ok(calc_lib::mod_(&list.borrow(), Some(&skill_cfg_of(st, sk)), &["TotemLife"]));
    let c = constants();
    let ally_life = monster_table_at(&c.monster_ally_life_table, level)
        .unwrap_or_else(|| panic!("Modules/CalcOffence.lua:340: attempt to perform arithmetic on a nil value (field '?')"));
    let mult = *c
        .totem_life_mult
        .0
        .get(&tostring_num(totem_id))
        .unwrap_or_else(|| panic!("Modules/CalcOffence.lua:340: attempt to perform arithmetic on a nil value (field '?')"));
    round((ally_life * mult).floor() * life_mod, None)
}

struct Totem {
    name: String,
    life: f64,
}

/// :3667-3790. `summoned_minion_override` is the local of :1504-1505 (`multiplierSummonedMinion`, nil when 0).
pub(super) fn ally_life(env: &mut Env, player: &mut ActorState, summoned_minion_override: Option<&ConfigValue>) {
    let mod_db = env.mod_db.clone();
    // :3668 `not skipEHP and ...` -- buildOutput (MAIN) never passes `skipEHP`.
    let needs_ally_life = base(&mod_db, &ALLY_LIFE_REDIRECTS) != 0.0;
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
        let mut notes = Vec::new();
        if let Some(st) = &env.skills {
            for (i, sk) in st.active_skill_list.iter().enumerate() {
                if sk.minion.is_none() || sk.flag("disable") || sk.skill_types.contains(&undamagable) {
                    continue;
                }
                let name = &skill_names[i];
                let skill_pool = if sk.flag("spectre") {
                    Some(&SPECTRE_LIFE_POOL)
                } else {
                    MINION_LIFE_POOL_BY_SKILL.iter().find(|(k, _)| k == name).map(|(_, p)| p)
                };
                let mut pools: Vec<&Pool> = skill_pool.into_iter().collect();
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
                // :3703-3750 `canAdd` for each pool; the first pool that can add needs `minion.output.Life` (R7).
                // `calculatedLifePool` / `spectreCount` only change after a pool is added, which needs that Life: here they stay empty / 0.
                for pool in pools {
                    let is_spectre = std::ptr::eq(pool, &SPECTRE_LIFE_POOL);
                    let can_add = ((is_spectre && 0.0 < spectre_limit) || !is_spectre)
                        && base(&mod_db, &[pool.redirect]) != 0.0
                        && !override_(&mod_db, &[pool.life]).is_some_and(|v| v.truthy());
                    if can_add {
                        notes.push(format!(
                            "CalcPerform.lua:3712-3729 Life of the minion of active skill {} ({name}) for {}: initMinionModDB / addMinionModifiers / \
                             doActorAttribsConditions / doActorLifeMana on the minion actor (R7)",
                            i + 1,
                            pool.life
                        ));
                        break;
                    }
                }
            }
        }
        env.unimplemented.extend(notes);
        // :3752-3758 the leftover spectre slots: `firstSpectre` is only set once a spectre pool was added (R7 above) -- nil here.

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
