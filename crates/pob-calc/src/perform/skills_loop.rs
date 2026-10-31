//! CalcPerform.lua:1294-1303 (every skill's perform-time `skillModList`) and :1334-1506 (the `activeSkillList` loop: brands, totems,
//! hexes, shocked ground, Bonechill, Skitterbots, guaranteed chills, minion counts, warcries, triggered-on-death, The Saviour; then the
//! summoned-minion multipliers) -- R6.5, skills on only.
//!
//! Hash-order walks (MINEFIELD-RUST R13): `pairs(minionCounts)` (:1492) is keyed by strings (`minionData.limit` or `<effect id>:<type>`):
//! LuaJIT's order differs per process; it only decides the insertion order of the `Multiplier:*Minion` mods (a multiset at the checkpoints),
//! so Rust takes the insertion order. Minions as actors (`activeSkill.minion.modDB`, `createMinionSkills`): [`crate::minion`].

use std::rc::Rc;

use indexmap::IndexMap;
use pob_mods::{ModCfg, ModStore, ModType, StoreRef, Value, create_mod};

use super::{Who, main_skill, nm, skill_guard, skill_type_u32};
use crate::active_skill::ActiveSkill;
use crate::env::Env;
use crate::output::Output;
use crate::setup::{n, new_mod, ok, s, tag};
use crate::skill_modlist::minion_data;

/// What the loop leaves for later phases.
#[derive(Default)]
pub(crate) struct LoopOut {
    /// `#` of :1491-1498 `summonedMinions` (the sum of `counts.total`).
    pub summoned_minions: f64,
    /// `guaranteedChillSkills` (:1430) -- indices into `activeSkillList`, read at :3379.
    pub guaranteed_chill_skills: Vec<usize>,
    /// `hasGuaranteedBonechill` (:1416/:1433), read at :3510 (the ailment phase, R6.9).
    pub has_guaranteed_bonechill: bool,
}

/// :1294-1303 `activeSkill.skillModList = new("ModList"):ModList(activeSkill.baseSkillModList)` for every skill; the minion half
/// (`activeSkill.minion.modDB`, `createMinionSkills`, `skillPartName`) is [`crate::minion::build_minion`] (R7.10). Also fills the mod core's `actor.activeSkillList` (the same Lua
/// table, read by `ModStore:GetStat` for `ManaReservedPercent`/`LifeReservedPercent`) with what it reads -- `skillData` as it is now.
pub(super) fn child_skill_lists(env: &mut Env) {
    let st = env.skills.as_ref().expect("skills on");
    let mut lists = Vec::with_capacity(st.active_skill_list.len());
    let mut core = Vec::with_capacity(st.active_skill_list.len());
    let mut notes = Vec::new();
    for sk in &st.active_skill_list {
        let base = sk
            .skill_mod_list
            .clone()
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:1296: activeSkill.baseSkillModList is nil"));
        lists.push(ModStore::new_list(Some(base)));
        core.push(pob_mods::ActiveSkill {
            skill_types: sk.skill_types.iter().map(|&t| f64::from(t)).collect(),
            disable: sk.skill_flags.get("disable").copied(),
            buff_list: Some(sk.buff_list.iter().filter_map(|b| b.name.as_str().map(Rc::from)).collect()),
            skill_data: sk
                .skill_data
                .iter()
                .filter_map(|(k, v)| v.as_num().map(|x| (Rc::from(k.as_str()), x)))
                .collect(),
        });
    }
    // :1297-1302 the minions: `minion.modDB`, `createMinionSkills`, `skillPartName` (R7.10, [`crate::minion::build_minion`])
    for (i, list) in lists.iter().enumerate() {
        if env.skills.as_ref().expect("skills on").active_skill_list[i].minion.is_some() {
            crate::minion::build_minion(env, i, list, &mut notes);
        }
    }
    env.skill_mod_lists = lists;
    env.player.borrow().actor.borrow_mut().active_skill_list = Some(core);
    env.unimplemented.extend(notes);
}

fn has_type(sk: &ActiveSkill, name: &str) -> bool {
    sk.skill_types.contains(&skill_type_u32(name))
}

fn data_truthy(sk: &ActiveSkill, k: &str) -> bool {
    sk.skill_data.get(k).is_some_and(Value::truthy)
}

/// The per-skill list queries, each behind the malformed-mod guard.
struct Sml<'a> {
    list: StoreRef,
    malformed: &'a [pob_data::skill_stat_map::MalformedMod],
}

impl Sml<'_> {
    fn sum(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().sum(ty, cfg, names))
    }

    fn more(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().more(cfg, names))
    }

    fn flag(&self, names: &[&str]) -> bool {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().flag(None, names)) == Some(true)
    }
}

/// `m_max(x, t[k] or 0)` written back.
fn max_mult(db: &StoreRef, key: &str, x: f64) {
    let cur = db.borrow().multipliers.get(key).copied().unwrap_or(0.0);
    db.borrow_mut().multipliers.insert(Rc::from(key), pob_lua::number::max2(x, cur));
}

#[derive(Default)]
struct Counts {
    total: Option<f64>,
    non_vaal: Option<f64>,
    permanent: Option<f64>,
}

/// :1334-1489 the loop, then :1491-1498 the summoned-minion multipliers.
#[allow(clippy::too_many_lines)]
pub(super) fn skills_loop(env: &mut Env, output: &mut Output) -> LoopOut {
    use ModType::{Base, Flag, Inc, More};
    use pob_lua::number::{max2, min2};
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let main = main_skill(env, Who::Player).expect("env.player.mainSkill");
    let mode_buffs = env.mode_buffs;
    let lists = env.skill_mod_lists.clone();
    let mut out = LoopOut::default();
    let mut minion_counts: IndexMap<String, Counts> = IndexMap::new();
    let mut notes = Vec::new();
    let st = env.skills.as_mut().expect("skills on");
    let skills = st.skills.clone();
    let non_damaging = |name: &str| -> f64 {
        pob_data::rules::rules()
            .non_damaging_ailment
            .get(name)
            .and_then(|a| a.default)
            .unwrap_or_else(|| panic!("data.nonDamagingAilment.{name}.default is nil"))
    };
    for (i, sk) in st.active_skill_list.iter_mut().enumerate() {
        let sml = Sml {
            list: lists[i].clone(),
            malformed: &sk.malformed_mods,
        };
        let cfg = sk
            .skill_cfg
            .as_ref()
            .map(|c| c.mod_cfg(&skills, &sk.skill_types, None))
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:1336: activeSkill.skillCfg is nil"));
        let ge = &skills.effects[sk.active_effect.borrow().granted_effect];
        let name = ge.name.as_str();
        let flag = |k: &str| sk.skill_flags.get(k).copied().unwrap_or(false);
        let mut data_writes: Vec<(&str, Value)> = Vec::new();

        // :1335-1347 brands
        if has_type(sk, "Brand") {
            let attach_limit = min2(
                sml.sum(Base, Some(&cfg), &["BrandsAttachedLimit"]),
                sml.sum(Base, Some(&cfg), &["ActiveBrandLimit"]),
            );
            let configured = super::base(&mod_db, &["Multiplier:ConfigBrandsAttachedToEnemy"]);
            let attached = if configured > 0.0 {
                min2(configured, attach_limit)
            } else {
                attach_limit
            };
            data_writes.push(("attachedBrandCount", n(attached)));
            let active_brands = super::base(&mod_db, &["Multiplier:ConfigActiveBrands"]);
            let limit = sml.sum(Base, Some(&cfg), &["ActiveBrandLimit"]);
            max_mult(&mod_db, "ActiveBrand", min2(active_brands, limit));
            max_mult(&mod_db, "BrandsAttachedToEnemy", attached);
            max_mult(&enemy_db, "BrandsAttached", attached);
        }
        // :1348-1353 totems -- the limit is read off the MAIN skill (upstream).
        if flag("totem") {
            main.guard(&["ActiveTotemLimit", "ActiveBallistaLimit"]);
            let limit = ok(main
                .list
                .borrow()
                .sum(Base, Some(&main.cfg), &["ActiveTotemLimit", "ActiveBallistaLimit"]));
            let atl = max2(limit, output.opt_num("ActiveTotemLimit").unwrap_or(0.0));
            output.set_num("ActiveTotemLimit", atl);
            let summoned = super::or_override(super::override_(&mod_db, &["TotemsSummoned"]), atl, "Modules/CalcPerform.lua:1351");
            output.set_num("TotemsSummoned", summoned);
            max_mult(&enemy_db, "TotemsSummoned", summoned);
        }
        // :1356-1366 hexes (`Sum(...)` is always a number, so the last clause of the condition is always true)
        if flag("hex") && flag("curse") && !has_type(sk, "TotemCastsWhenNotDetached") && {
            sml.sum(Base, None, &["MaxDoom"]);
            true
        } {
            let hex_doom = super::base(&mod_db, &["Multiplier:HexDoomStack"]);
            let max_doom = sml.sum(Base, None, &["MaxDoom"]);
            let doom_effect = sml.more(None, &["DoomEffect"]);
            let limit = max2(max_doom, output.opt_num("HexDoomLimit").unwrap_or(0.0));
            output.set_num("HexDoomLimit", limit);
            nm(
                &sml.list,
                "CurseEffect",
                Inc,
                min2(hex_doom, max_doom) * doom_effect,
                "Doom",
                0,
                0,
                vec![],
            );
            let cur = mod_db.borrow().multipliers.get("HexDoom").copied().unwrap_or(0.0);
            mod_db
                .borrow_mut()
                .multipliers
                .insert(Rc::from("HexDoom"), min2(max2(hex_doom, cur), limit));
        }
        // :1367-1371 shocked ground
        if name == "Vaal Lightning Trap" || name == "Shock Ground" {
            let effect = sml.sum(Base, None, &["ShockedGroundEffect"]) * (1.0 + sml.sum(Inc, None, &["EnemyShockEffect"]) / 100.0);
            nm(
                &mod_db,
                "ShockOverride",
                Base,
                effect,
                "Shocked Ground",
                0,
                0,
                vec![tag(&[
                    ("type", s("ActorCondition")),
                    ("actor", s("enemy")),
                    ("var", s("OnShockedGround")),
                ])],
            );
        }
        // :1372-1374
        if data_truthy(sk, "supportBonechill")
            && (has_type(sk, "ChillingArea") || has_type(sk, "NonHitChill") || !sml.flag(&["CannotChill"]))
        {
            output.set_bool("HasBonechill", true);
        }
        if name == "Summon Skitterbots" {
            // :1375-1426
            let skill_src = crate::setup::cfg_source("Skill");
            let ailment_effect = sml.sum(Inc, None, &["SkitterbotAilmentEffect"]);
            let affect_player = || sml.flag(&["SkitterbotAffectPlayer"]);
            if !sml.flag(&["SkitterbotsCannotShock"]) {
                let effect =
                    non_damaging("Shock") * (1.0 + (sml.sum(Inc, Some(&skill_src), &["EnemyShockEffect"]) + ailment_effect) / 100.0);
                nm(&mod_db, "ShockOverride", Base, effect, name, 0, 0, vec![]);
                new_mod(&enemy_db, "Condition:Shocked", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                if affect_player() {
                    new_mod(&mod_db, "Shock", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                    nm(&mod_db, "SelfShockOverride", Base, effect, name, 0, 0, vec![]);
                }
            }
            if !sml.flag(&["SkitterbotsCannotChill"]) {
                let effect =
                    non_damaging("Chill") * (1.0 + (sml.sum(Inc, Some(&skill_src), &["EnemyChillEffect"]) + ailment_effect) / 100.0);
                nm(&mod_db, "ChillOverride", Base, effect, name, 0, 0, vec![]);
                new_mod(&enemy_db, "Condition:Chilled", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                if affect_player() {
                    new_mod(&mod_db, "Chill", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                    nm(&mod_db, "SelfChillOverride", Base, effect, name, 0, 0, vec![]);
                }
                if data_truthy(sk, "supportBonechill") {
                    out.has_guaranteed_bonechill = true;
                    new_mod(&mod_db, "SkitterbotBonechill", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                }
            }
            if sml.flag(&["ScorchingSkitterbot"]) {
                let effect =
                    non_damaging("Scorch") * (1.0 + (sml.sum(Inc, Some(&skill_src), &["EnemyScorchEffect"]) + ailment_effect) / 100.0);
                nm(&mod_db, "ScorchOverride", Base, effect, name, 0, 0, vec![]);
                new_mod(&enemy_db, "Condition:Scorched", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                if affect_player() {
                    new_mod(&mod_db, "Scorch", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
                    nm(&mod_db, "SelfScorchOverride", Base, effect, name, 0, 0, vec![]);
                }
            }
        } else if flag("guaranteedChill") || has_type(sk, "ChillingArea") || (has_type(sk, "NonHitChill") && !sml.flag(&["CannotChill"])) {
            // :1428-1435
            out.guaranteed_chill_skills.push(i);
            new_mod(&enemy_db, "Condition:Chilled", Flag, Value::Bool(true), Some(name), 0, 0, vec![]);
            if data_truthy(sk, "supportBonechill") {
                out.has_guaranteed_bonechill = true;
            }
        }
        // :1436-1463 count active minions; skills without a limit contribute one minion.
        if !flag("disable")
            && let Some(minion_list) = sk.minion_list.as_ref().filter(|l| !l.is_empty())
        {
            for minion_type in minion_list {
                let md = minion_data(minion_type);
                if md.is_nil() || md.get("hostile").truthy() {
                    continue;
                }
                let limit = md.get("limit");
                let limit_name = limit.as_str().map(str::to_owned);
                if !limit.is_nil() && limit_name.is_none() {
                    panic!("Modules/CalcPerform.lua:1443: minionData.limit is not a string ({limit:?})");
                }
                let key = limit_name.clone().unwrap_or_else(|| format!("{}:{minion_type}", ge.id));
                let mut count = 1.0;
                if let Some(ln) = &limit_name {
                    let ov = super::override_(&mod_db, &[ln]);
                    count = match ov {
                        Some(v) if v.truthy() => super::num_val(&v, "Modules/CalcPerform.lua:1446"),
                        _ => {
                            skill_guard(sml.malformed, &[ln]);
                            let val = ok(crate::calc_lib::val(&sml.list.borrow(), ln, None));
                            val * sml.more(Some(&cfg), &["ActiveMinionLimit"])
                        }
                    }
                    .floor();
                    let cur = output.opt_num(ln).unwrap_or(0.0);
                    output.set_num(ln, max2(count, cur));
                }
                let counts = minion_counts.entry(key).or_default();
                counts.total = Some(max2(count, counts.total.unwrap_or(0.0)));
                if !has_type(sk, "Vaal") {
                    counts.non_vaal = Some(max2(count, counts.non_vaal.unwrap_or(0.0)));
                }
                if flag("permanentMinion") {
                    counts.permanent = Some(max2(count, counts.permanent.unwrap_or(0.0)));
                }
            }
        }
        // :1464-1466
        if has_type(sk, "CreatesMinion") && !has_type(sk, "MinionsAreUndamagable") {
            new_mod(
                &mod_db,
                "Condition:HaveDamageableMinion",
                Flag,
                Value::Bool(true),
                None,
                0,
                0,
                vec![],
            );
        }
        // :1467-1475 warcries
        if mode_buffs && flag("warcry") {
            if name == "Rallying Cry" && !sml.flag(&["CannotShareWarcryBuffs"]) && !super::flag(&mod_db, &["RallyingActive"]) {
                let v = sml.sum(Base, Some(&main.cfg), &["RallyingCryExertDamageBonus"]);
                new_mod(&mod_db, "RallyingExertMoreDamagePerAlly", Base, n(v), None, 0, 0, vec![]);
                new_mod(&mod_db, "RallyingActive", Flag, Value::Bool(true), None, 0, 0, vec![]);
            } else if name == "Seismic Cry" && !super::flag(&mod_db, &["SeismicActive"]) {
                let v = sml.sum(Base, Some(&main.cfg), &["SeismicAoEMoreMultiplier"]);
                new_mod(&mod_db, "SeismicMoreAoE", Base, n(v), None, 0, 0, vec![]);
                new_mod(&mod_db, "SeismicActive", Flag, Value::Bool(true), None, 0, 0, vec![]);
            }
        }
        // :1476-1486 triggered on death
        if data_truthy(sk, "triggeredOnDeath") && !flag("minion") {
            data_writes.push(("triggered", Value::Bool(true)));
            for ty in [Inc, More] {
                skill_guard(sml.malformed, &["TriggeredDamage"]);
                let entries = ok(sml.list.borrow().tabulate(Some(ty), Some(&main.cfg), &["TriggeredDamage"]));
                for e in entries {
                    let m = &e.mod_;
                    let created = create_mod(
                        "Damage",
                        Some(ty),
                        m.value.clone(),
                        m.source.as_deref(),
                        m.flags,
                        m.keyword_flags,
                        m.tags.clone(),
                    );
                    sml.list.borrow_mut().new_mod(created);
                }
            }
            data_writes.push(("triggerTime", n(60.0 * 1000.0)));
        }
        // :1487-1490 The Saviour: `infoMessage` / `infoTrigger` are read by the trigger/offence phase (R7).
        if name == "Reflection" || data_truthy(sk, "triggeredBySaviour") {
            notes.push(format!(
                "CalcPerform.lua:1488-1489 activeSkill.infoMessage/infoTrigger of skill {} ({name}) (read by CalcTriggers/offence, R7)",
                i + 1
            ));
        }
        for (k, v) in data_writes {
            sk.skill_data.insert(k.to_owned(), v);
        }
    }

    // :1491-1498
    let combat = || tag(&[("type", s("Condition")), ("var", s("Combat"))]);
    for counts in minion_counts.values() {
        let total = counts.total.expect("counts.total");
        out.summoned_minions += total;
        nm(&mod_db, "Multiplier:SummonedMinion", Base, total, "Config", 0, 0, vec![combat()]);
        if let Some(v) = counts.non_vaal {
            nm(&mod_db, "Multiplier:NonVaalSummonedMinion", Base, v, "Config", 0, 0, vec![combat()]);
        }
        if let Some(v) = counts.permanent {
            nm(&mod_db, "Multiplier:PermanentMinion", Base, v, "Config", 0, 0, vec![combat()]);
        }
    }
    env.unimplemented.extend(notes);
    out
}
