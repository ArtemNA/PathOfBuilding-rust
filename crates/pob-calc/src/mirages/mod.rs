//! `calcs.mirages(env)` -- Modules/CalcMirages.lua (428 lines; `grep -n` on upstream `ed354c2f8`, 2026-11-01): `calculateMirage` (:24-58) and the
//! five branches of `calcs.mirages` (:60-428) -- Mirage Archer (:67-119), The Saviour / Reflection (:120-164), Tawhoa's Chosen (:165-283), Sacred
//! Wisps (:284-345), General's Cry (:346-425) -- with `calcs.copyActiveSkill` (CalcActiveSkill.lua:167-192). Called at CalcPerform.lua:3798,
//! after `calcs.triggers`; `true` = offence is not run for `env.player.mainSkill` (:3798-3800).
//!
//! Upstream shapes kept (R7-ARCH §4, R7.9):
//! * The copy is a skill OUTSIDE `activeSkillList` ([`crate::skills::SkillsState::detached_main`], reached through `mainSkill` only): perform's
//!   loops never see it (its `skillModList` is not rebuilt at CalcPerform.lua:1295-1296 -- the `preCalcFunc` mods stay), its list's parent is
//!   the modDB of the COPIED skill's actor (another env's player, CalcActiveSkill.lua:492), its `SkillData` and `auxSkillList` are the new
//!   env's (:661/:873). The new env is a fresh CALCULATOR initEnv (`buildBreakdown` false, R8), performed with `skipEHP` nil, and cached
//!   (`cacheData` ends every perform).
//! * `env.player.output = newEnv.player.output` (:157/:235) shares one table between the two envs; here the new run's output is copied into
//!   `env`'s ([`crate::output::Output::copy_for`]) and the four Tawhoa writes (:236-239) go to both. What writes into `env`'s output after this
//!   (the buildOutput tail, Calcs.lua:395-437) does not reach the mirage run's copy -- nothing reads the mirage run's output afterwards except
//!   `calcs.triggers` / `getCachedOutputValue`, which rebuild in CALCULATOR mode (CalcTriggers.lua:80, CalcPerform.lua:33).
//! * `activeSkill.mirage.output` keeps the mirage run's scalars ([`crate::active_skill::Mirage`]).
//! * `cachedData[mode][uuid].ActiveSkill` is read through [`CalcSession::cached_active_skill`] (the R7.2b mine: a cached run that is the env
//!   being performed in place resolves to the env in hand).
//! * `env.player.mainSkill.ModFlags` / `.KeywordFlags` (:85-86, :146, :150, :229, :311-312) are not fields of an active skill: nil, so the
//!   `NewMod`s get flags 0, keywordFlags 0, no tags (ModTools.lua:33-49).
//! * Boundaries (an `Err`, no offence, recorded): a copy of a skill with a minion (`createMinionSkills` on the copy, :185-190 -- R7.10) and a
//!   cached run that stopped at a boundary itself.
//! * Hash order: none (`ipairs` over `activeSkillList`, `Tabulate` lists).

use std::rc::Rc;

use pob_lua::number::{max2, round};
use pob_lua::tostring_num;
use pob_mods::{ModCfg, ModStore, ModType, StoreRef, Value, create_mod};

use crate::active_skill::{ActiveSkill, ActorView, EffectRef, Mirage, SkillEffect, create_active_skill};
use crate::calc_tools::WeaponView;
use crate::env::{CalcActor, Env};
use crate::offence::{Hook, Q};
use crate::perform::{PerformState, skill_cfg_of, skill_list_of};
use crate::session::{CachedActiveSkill, CalcMode, CalcSession, RunRef, cache_skill_uuid, is_limited};
use crate::setup::{misc, mod_flag};
use crate::skills::{DETACHED, DetachedSkill, SkillsState};

const CM: &str = "Modules/CalcMirages.lua";

fn st(env: &Env) -> &SkillsState {
    env.skills.as_ref().expect("skills on")
}

fn st_mut(env: &mut Env) -> &mut SkillsState {
    env.skills.as_mut().expect("skills on")
}

/// `skill.activeEffect.grantedEffect.name`.
fn name_of_skill(s: &SkillsState, sk: &ActiveSkill) -> String {
    s.skills.effects[sk.active_effect.borrow().granted_effect].name.clone()
}

/// `skill.skillTypes[SkillType[name]]`. A name that is not a `SkillType` (`SkillType.Totem`, :125/:184 -- the data has `SummonsTotem` /
/// `UsedByTotem` only) is `skillTypes[nil]`: a read of the nil key, nil.
fn has_type(sk: &ActiveSkill, name: &str) -> bool {
    pob_data::globals::globals()
        .skill_type
        .get(name)
        .is_some_and(|&t| sk.skill_types.contains(&(t as u32)))
}

/// `skill.skillCfg.skillCond["usedByMirage"]`.
fn used_by_mirage(sk: &ActiveSkill) -> bool {
    sk.skill_cfg
        .as_ref()
        .is_some_and(|c| c.skill_cond.get("usedByMirage").copied().unwrap_or(false))
}

fn sd_truthy(sk: &ActiveSkill, k: &str) -> bool {
    sk.skill_data.get(k).is_some_and(Value::truthy)
}

/// `skill.skillData[k] or 0` used as a number.
fn sd_or0(sk: &ActiveSkill, k: &str, line: u32) -> f64 {
    match sk.skill_data.get(k) {
        Some(v) if v.truthy() => v.as_num().unwrap_or_else(|| {
            panic!(
                "{CM}:{line}: attempt to perform arithmetic on a {} value (skillData.{k})",
                v.type_name()
            )
        }),
        _ => 0.0,
    }
}

/// `env.player.activeSkillList[i + 1].skillModList` / `.skillCfg` (or the main skill's).
fn q_of(env: &Env, i: usize) -> Q {
    Q {
        list: skill_list_of(env, i),
        malformed: st(env).skill(i).malformed_mods.clone(),
    }
}

fn cfg_of(env: &Env, i: usize) -> ModCfg {
    let s = st(env);
    skill_cfg_of(s, s.skill(i))
}

/// The new env's detached skill (`newSkill`).
fn new_skill(new_env: &Env) -> &DetachedSkill {
    st(new_env).detached_main.as_ref().expect("newSkill")
}

fn new_skill_mut(new_env: &mut Env) -> &mut DetachedSkill {
    st_mut(new_env).detached_main.as_mut().expect("newSkill")
}

/// `newSkill.skillModList` / `newSkill.skillCfg`.
fn new_q(new_env: &Env) -> Q {
    let d = new_skill(new_env);
    Q {
        list: d.skill_mod_list.clone(),
        malformed: d.skill.malformed_mods.clone(),
    }
}

fn new_cfg(new_env: &Env) -> ModCfg {
    skill_cfg_of(st(new_env), &new_skill(new_env).skill)
}

/// `list:NewMod(name, type, value, source, nil, nil)` -- flags 0, keywordFlags 0, no tags (see the module doc).
fn nm(list: &StoreRef, name: &str, ty: ModType, value: Value, source: &str) {
    list.borrow_mut()
        .new_mod(create_mod(name, Some(ty), value, Some(source), 0, 0, Vec::new()));
}

fn session(env: &Env) -> Result<CalcSession, String> {
    CalcSession::of(env).ok_or_else(|| format!("{CM}: calcs.mirages outside a CalcSession"))
}

/// A skill `calculateMirage` copies: `env.player.mainSkill` / an `activeSkillList` entry of the env in hand, or a cached run's `ActiveSkill`.
#[derive(Clone)]
enum Src {
    Current(usize),
    Listed(RunRef, usize),
    Detached(Rc<DetachedSkill>),
}

impl From<CachedActiveSkill> for Src {
    fn from(c: CachedActiveSkill) -> Src {
        match c {
            CachedActiveSkill::Listed(r, i) => Src::Listed(r, i),
            CachedActiveSkill::Current(i) => Src::Current(i),
            CachedActiveSkill::Detached(d) => Src::Detached(d),
        }
    }
}

/// CalcTriggers.lua:1603 for a detached main skill: `if actor.mainSkill.skillFlags.disable or env.limitedSkills[cacheSkillUUID(mainSkill)]
/// then return end`. Every mirage marks its copy in the new env's `limitedSkills` (CalcMirages.lua:44-45), so `calcs.triggers` returns there
/// with no effect; a detached main skill that is neither is a boundary (it cannot be made by this port).
pub(crate) fn triggers_on_detached(env: &Env) -> Result<(), String> {
    if st(env).skill(DETACHED).flag("disable") || is_limited(env, DETACHED) {
        Ok(())
    } else {
        Err("CalcTriggers.lua:1602-1627 calcs.triggers on a mirage's copy that is not in env.limitedSkills (R7.9)".into())
    }
}

/// What `calcs.copyActiveSkill` reads of the copied skill (`skill.activeEffect`, `supportList`, `socketGroup`, `summonSkill`, `actor`) and the
/// cached run that owns that actor (see [`DetachedSkill::keep`]).
struct Copied {
    active_effect: EffectRef,
    support_list: Vec<EffectRef>,
    socket_group: Option<usize>,
    summon_skill: Option<usize>,
    actor: Rc<std::cell::RefCell<CalcActor>>,
    keep: Option<RunRef>,
}

/// `GlobalCache.cachedData[mode][uuid]`'s `CritChance`, `TotalDPS`, `Speed` (the scalar copies of `cacheData`).
type CachedScalars = (Option<f64>, Option<f64>, Option<f64>);

/// CalcActiveSkill.lua:167-192 `calcs.copyActiveSkill(env, "CALCULATOR", skill)` -> `(newSkill, newEnv)`; `newSkill` is put into
/// `newEnv` as its [`SkillsState::detached_main`] (`mainSkill` is set later, CalcMirages.lua:51).
fn copy_active_skill(env: &Env, src: &Src) -> Result<Env, String> {
    let sess = session(env)?;
    // the copied skill's fields and its actor
    let of = |sk: &ActiveSkill, actor: Rc<std::cell::RefCell<CalcActor>>, keep: Option<RunRef>| Copied {
        active_effect: sk.active_effect.clone(),
        support_list: sk.support_list.clone(),
        socket_group: sk.socket_group,
        summon_skill: sk.summon_skill,
        actor,
        keep,
    };
    let Copied {
        active_effect,
        support_list,
        socket_group,
        summon_skill,
        actor,
        keep,
    } = match src {
        Src::Current(i) => match st(env).detached_main.as_ref().filter(|_| *i == DETACHED) {
            Some(d) => of(&d.skill, d.actor.clone(), d.keep.clone()),
            None => of(st(env).skill(*i), env.player.clone(), None),
        },
        Src::Listed(run, i) => {
            let r = run.borrow();
            of(st(&r.env).skill(*i), r.env.player.clone(), Some(run.clone()))
        }
        Src::Detached(d) => of(&d.skill, d.actor.clone(), d.keep.clone()),
    };
    if summon_skill.is_some() {
        return Err("CalcActiveSkill.lua:181 copyActiveSkill of a minion skill (summonSkill: the minion actor, R7.10 boundary)".into());
    }
    // :182 `calcs.initEnv(env.build, mode, env.override)` -- no override in this port
    let mut new_env = match sess.init_env(CalcMode::Calculator) {
        Ok(e) => e,
        Err(crate::session::SessionError::Calc(e)) => return Err(format!("CalcActiveSkill.lua:182 initEnv: {}", e.0)),
        Err(crate::session::SessionError::Unimplemented(w)) => return Err(format!("CalcActiveSkill.lua:182 initEnv: {w}")),
    };
    let mut nst = new_env.skills.take().expect("skills on");
    // :168-179 the new activeEffect: `srcInstance` is the gem instance of the shared `build.skillsTab` (here: the new env's copy of it)
    let (ge, level, quality, src_inst) = {
        let e = active_effect.borrow();
        (e.granted_effect, e.level, e.quality, e.src_instance)
    };
    let mut eff = SkillEffect {
        granted_effect: ge,
        level,
        quality,
        ..SkillEffect::default()
    };
    let mut src_from_item = false;
    if let Some((g, k)) = src_inst {
        let gi = &nst.tab.socket_group_list()[g].gem_list[k];
        eff.level = gi.level;
        eff.quality = gi.quality;
        eff.src_instance = Some((g, k));
        eff.gem_data = gi.gem_data.clone();
        src_from_item = gi.from_item == Some(true);
    }
    // :181 `calcs.createActiveSkill(activeEffect, skill.supportList, skill.actor, skill.socketGroup, skill.summonSkill)`
    let mut skill = {
        let a = actor.borrow();
        let (t1, t2) = (a.weapon_data1.get("type"), a.weapon_data2.get("type"));
        let view = ActorView {
            weapon1: WeaponView {
                ty: t1.as_str(),
                counts_as_all_1h: a.weapon_data1.get("countsAsAll1H").truthy(),
            },
            weapon2: WeaponView {
                ty: t2.as_str(),
                counts_as_all_1h: a.weapon_data2.get("countsAsAll1H").truthy(),
            },
            is_player: true,
        };
        create_active_skill(
            &nst.skills,
            EffectRef::new(eff),
            support_list,
            &view,
            socket_group,
            src_from_item,
            &nst.from_item_marks,
        )
        .map_err(|e| format!("CalcActiveSkill.lua:181 createActiveSkill: {e}"))?
    };
    // :183 `calcs.buildActiveSkillModList(newEnv, newSkill)` -- the new env's inputs, the copied skill's actor
    let mut notes = Vec::new();
    let aux = {
        let sk = nst.skills.clone();
        let a = actor.borrow();
        let sa = crate::skill_modlist::SkillActor {
            mod_db: &a.mod_db,
            item_list: &a.item_list,
            weapon_data1: &a.weapon_data1,
            weapon_data2: &a.weapon_data2,
            minion: None,
            summon_name: None,
        };
        crate::init_env::with_mod_list_inputs(&new_env, |inp| {
            crate::skill_modlist::build_skill(&sk, &mut nst.tab, &mut skill, inp, &sa, &mut notes)
        })
        .map_err(|e| format!("CalcActiveSkill.lua:183 buildActiveSkillModList: {e}"))?
    };
    if aux {
        nst.aux_skill_list.push(DETACHED);
    }
    new_env.unimplemented.extend(notes);
    // :185-190 a copy with a minion: `createMinionSkills(env, newSkill)` on the copy (the minion actor of a mirage, R7.10)
    if skill.minion.is_some() {
        return Err(
            "CalcActiveSkill.lua:185-190 copyActiveSkill of a skill with a minion (createMinionSkills on the copy, R7.10 boundary)".into(),
        );
    }
    // :184 `newSkill.skillModList = new("ModList"):ModList(newSkill.baseSkillModList)`
    let base = skill
        .skill_mod_list
        .clone()
        .unwrap_or_else(|| panic!("Modules/CalcActiveSkill.lua:184: newSkill.baseSkillModList is nil"));
    nst.detached_main = Some(DetachedSkill {
        skill,
        skill_mod_list: ModStore::new_list(Some(base)),
        actor,
        keep,
    });
    new_env.skills = Some(nst);
    Ok(new_env)
}

/// What the branch of `calcs.mirages` is (`config`, :68-425).
enum Kind {
    Archer,
    /// `maxMirageWarriors` (:122), halved by `preCalcFunc` when both weapons share a name (:147-149).
    Saviour {
        max_warriors: f64,
        best: Option<f64>,
    },
    Tawhoa(Box<Tawhoa>),
    Wisps,
}

/// The locals of the Tawhoa's Chosen branch (:166-178).
#[derive(Default)]
struct Tawhoa {
    best: Option<f64>,
    effective_source_rate: Option<f64>,
    trigger_rate_cap: f64,
    skill_trigger_rate: f64,
}

/// :60-428 `calcs.mirages(env)`: `Ok(true)` = skip `calcs.offence` for `env.player.mainSkill` (:3798); `Err` = a boundary of the port.
pub(crate) fn mirages(env: &mut Env, st_: &mut PerformState, hook: Hook<'_, '_>) -> Result<bool, String> {
    let Some(m) = st(env).main_skill else { return Ok(false) };
    let main = st(env).skill(m);
    // :63-65
    if used_by_mirage(main) || main.flag("disable") {
        return Ok(false);
    }
    let name = name_of_skill(st(env), main);
    let kind = if sd_truthy(main, "triggeredByMirageArcher") {
        // :67-119
        Kind::Archer
    } else if name == "Reflection" {
        // :120-164; :122
        let max_warriors = q_of(env, m).base(Some(&cfg_of(env, m)), &["SaviourMirageWarriorMaxCount"]);
        Kind::Saviour { max_warriors, best: None }
    } else if name == "Tawhoa's Chosen" {
        // :165-283
        Kind::Tawhoa(Box::default())
    } else if sd_truthy(main, "triggeredBySacredWisps") {
        // :284-345
        Kind::Wisps
    } else if sd_truthy(main, "triggeredByGeneralsCry") {
        // :346-425, then :427 `calculateMirage(env, nil)` returns nil
        return generals_cry(env, m);
    } else {
        // :427 `calculateMirage(env, nil)` -> nil
        return Ok(false);
    };
    calculate_mirage(env, st_, hook, m, kind)
}

/// :24-58 `calculateMirage(env, config)`.
fn calculate_mirage(env: &mut Env, pst: &mut PerformState, hook: Hook<'_, '_>, m: usize, mut kind: Kind) -> Result<bool, String> {
    // :29 `config.mirageSkill` (Mirage Archer / Sacred Wisps: the main skill)
    let mut mirage_skill: Option<Src> = match kind {
        Kind::Archer | Kind::Wisps => Some(Src::Current(m)),
        _ => None,
    };
    // :31-37 `config.compareFunc` over `ipairs(env.player.activeSkillList)`
    if matches!(kind, Kind::Saviour { .. } | Kind::Tawhoa(_)) {
        let n = st(env).active_skill_list.len();
        for j in 0..n {
            if !used_by_mirage(&st(env).active_skill_list[j]) {
                mirage_skill = match &mut kind {
                    Kind::Saviour { best, .. } => saviour_compare(env, m, j, mirage_skill, best)?,
                    Kind::Tawhoa(t) => tawhoa_compare(env, m, j, mirage_skill, t)?,
                    _ => unreachable!("compareFunc of Archer / Wisps"),
                };
            }
        }
    }
    let calc_main_skill_offence = matches!(kind, Kind::Archer | Kind::Wisps);
    let Some(src) = mirage_skill else {
        // :55 `config.mirageSkillNotFoundFunc(env, config)`
        let sk = st_mut(env).skill_mut(m);
        match kind {
            Kind::Archer => {
                if sk.info_message2.is_none() {
                    sk.info_message2 = Some("No Mirage Archer active skill found".into());
                }
            }
            Kind::Saviour { .. } => {
                sk.disable_reason = Some("No Saviour active skill found".into());
                sk.skill_flags.insert("disable".into(), true);
            }
            Kind::Tawhoa(_) => {
                sk.disable_reason = Some("No Tawhoa's Chosen active skill found".into());
                sk.skill_flags.insert("disable".into(), true);
            }
            Kind::Wisps => {
                if sk.info_message2.is_none() {
                    sk.info_message2 = Some("No active skill for Sacred Wisps found".into());
                }
            }
        }
        // :57
        return Ok(!calc_main_skill_offence);
    };
    // :40
    let mut new_env = copy_active_skill(env, &src)?;
    {
        let d = new_skill_mut(&mut new_env);
        // :41
        d.skill
            .skill_cfg
            .as_mut()
            .unwrap_or_else(|| panic!("{CM}:41: attempt to index field 'skillCfg' (a nil value)"))
            .skill_cond
            .insert("usedByMirage".into(), true);
        // :42-43
        d.skill.skill_flags.shift_remove("multiPart");
        d.skill.skill_flags.shift_remove("haveMinion");
    }
    // :44-45 `newEnv.limitedSkills = newEnv.limitedSkills or {}`; `[cacheSkillUUID(newSkill, newEnv)] = true`
    let uuid = cache_skill_uuid(&new_env, DETACHED);
    new_env.limited_skills.get_or_insert_with(Default::default).insert(uuid);
    // :46 `newSkill.skillData.mirageUses = env.player.mainSkill.skillData.storedUses`
    let stored = st(env).skill(m).skill_data.get("storedUses").cloned();
    {
        let d = new_skill_mut(&mut new_env);
        match stored {
            Some(v) if !v.is_nil() => {
                d.skill.skill_data.insert("mirageUses".into(), v);
            }
            _ => {
                d.skill.skill_data.shift_remove("mirageUses");
            }
        }
        // :47
        d.skill.skill_types.insert(crate::offence::skill_type_u32("OtherThingUsesSkill"));
    }
    // :49 `config.preCalcFunc(env, newSkill, newEnv)`
    match &mut kind {
        Kind::Archer => archer_pre(env, &new_env, m),
        Kind::Saviour { max_warriors, .. } => saviour_pre(env, &new_env, m, max_warriors),
        Kind::Tawhoa(t) => tawhoa_pre(env, &mut new_env, m, t),
        Kind::Wisps => wisps_pre(env, &new_env, m)?,
    }
    // :51 `newEnv.player.mainSkill = newSkill`
    st_mut(&mut new_env).main_skill = Some(DETACHED);
    // :52 `calcs.perform(newEnv)`
    let sess = session(env)?;
    let run = sess.perform_hooked(new_env, false, hook);
    if !run.borrow().offence_ran {
        let why = run
            .borrow()
            .env
            .unimplemented
            .iter()
            .find(|u| u.starts_with("CalcTriggers.lua:") || u.starts_with("CalcMirages.lua:") || u.starts_with("CalcActiveSkill.lua:"))
            .cloned()
            .unwrap_or_else(|| "calcs.offence did not run".into());
        return Err(format!("{CM}:52 the mirage's perform stopped at a boundary of the port: {why}"));
    }
    // :53 `config.postCalcFunc(env, newSkill, newEnv)`
    match kind {
        Kind::Archer => mirage_post(env, &run, m, "mirageArcher"),
        Kind::Wisps => mirage_post(env, &run, m, "wisp"),
        Kind::Saviour { max_warriors, .. } => {
            // :155-158
            replace_main(env, pst, &run);
            let name = name_of_skill(st(env), st(env).skill(DETACHED));
            st_mut(env).skill_mut(DETACHED).info_message = Some(format!("{} Mirage Warriors using {name}", tostring_num(max_warriors)));
        }
        Kind::Tawhoa(t) => {
            // :232-239
            replace_main(env, pst, &run);
            let name = name_of_skill(st(env), st(env).skill(DETACHED));
            st_mut(env).skill_mut(DETACHED).info_message = Some(format!("Tawhoa's Chosen using {name}"));
            let mut r = run.borrow_mut();
            for o in [&mut pst.player.output, &mut r.state.player.output] {
                o.set_num("Speed", t.skill_trigger_rate);
                o.set_num("TriggerRateCap", t.trigger_rate_cap);
                match t.effective_source_rate {
                    Some(v) => o.set_num("EffectiveSourceRate", v),
                    None => o.remove("EffectiveSourceRate"),
                }
                o.set_num("SkillTriggerRate", t.skill_trigger_rate);
            }
        }
    }
    // :57
    Ok(!calc_main_skill_offence)
}

/// :155 `env.player.mainSkill = newSkill` and :157 / :235 `env.player.output = newEnv.player.output` (see the module doc).
fn replace_main(env: &mut Env, pst: &mut PerformState, run: &RunRef) {
    let r = run.borrow();
    let d = st(&r.env).detached_main.clone().expect("newSkill");
    let actor = env.player.borrow().actor.clone();
    pst.player.output = r.state.player.output.copy_for(&actor);
    let s = st_mut(env);
    s.detached_main = Some(d);
    s.main_skill = Some(DETACHED);
}

/// :99-113 / :325-339 `postCalcFunc` of Mirage Archer / Sacred Wisps: `mirage.output`, `skillFlags[flag] = true` (the minion half is the
/// R7.10 boundary of [`copy_active_skill`]; breakdown is R8).
fn mirage_post(env: &mut Env, run: &RunRef, m: usize, flag: &str) {
    let out = Rc::new(run.borrow().state.player.output.scalars());
    let sk = st_mut(env).skill_mut(m);
    sk.mirage.get_or_insert_with(Mirage::default).output = Some(out);
    sk.skill_flags.insert(flag.into(), true);
}

/// The `skillPart` lines of :91-97 / :317-323.
fn mirage_skill_part(new_env: &Env, mirage: &mut Mirage) {
    let d = &new_skill(new_env).skill;
    if d.skill_part_name.is_some() {
        mirage.skill_part = d.skill_part;
        mirage.skill_part_name.clone_from(&d.skill_part_name);
        mirage.info_message2 = Some(name_of_skill(st(new_env), d));
    } else {
        mirage.skill_part_name = None;
    }
}

/// :71-98 Mirage Archer `preCalcFunc`.
fn archer_pre(env: &mut Env, new_env: &Env, m: usize) {
    let (nq, ncfg) = (new_q(new_env), new_cfg(new_env));
    let more_damage = nq.base(Some(&ncfg), &["MirageArcherLessDamage"]);
    let more_attack_speed = nq.base(Some(&ncfg), &["MirageArcherLessAttackSpeed"]);
    let mirage_count = nq.base(Some(&cfg_of(env, m)), &["MirageArcherMaxCount"]);
    let new_name = name_of_skill(st(new_env), &new_skill(new_env).skill);
    let mut mirage = Mirage {
        name: new_name.clone(),
        count: Some(mirage_count),
        ..Mirage::default()
    };
    {
        let sk = st_mut(env).skill_mut(m);
        if sk.info_message.is_none() {
            sk.info_message = Some(format!("{} Mirage Archers using {new_name}", tostring_num(mirage_count)));
        }
    }
    nm(&nq.list, "Damage", ModType::More, Value::Num(more_damage), "Mirage Archer");
    nm(&nq.list, "Speed", ModType::More, Value::Num(more_attack_speed), "Mirage Archer");
    nm(&nq.list, "HasNoCost", ModType::Flag, Value::Bool(true), "Used by mirage");
    mirage_skill_part(new_env, &mut mirage);
    st_mut(env).skill_mut(m).mirage = Some(mirage);
}

/// :288-324 Sacred Wisps `preCalcFunc`.
fn wisps_pre(env: &mut Env, new_env: &Env, m: usize) -> Result<(), String> {
    let nq = new_q(new_env);
    let main_cfg = cfg_of(env, m);
    let less_damage = nq.base(Some(&main_cfg), &["SacredWispsLessDamage"]);
    // :294-300 the Wisps summoning skill
    let (mut chance, mut max_count) = (None, None);
    let n = st(env).active_skill_list.len();
    for j in 0..n {
        if name_of_skill(st(env), &st(env).active_skill_list[j]) == "Summon Sacred Wisps" {
            let q = q_of(env, j);
            chance = Some(q.base(Some(&main_cfg), &["SacredWispsChance"]));
            max_count = Some(q.base(Some(&main_cfg), &["SacredWispsMaxCount"]));
            break;
        }
    }
    let new_name = name_of_skill(st(new_env), &new_skill(new_env).skill);
    let mut mirage = Mirage {
        name: new_name.clone(),
        count: max_count,
        ..Mirage::default()
    };
    {
        let sk = st_mut(env).skill_mut(m);
        if sk.info_message.is_none() {
            let c = max_count.map_or_else(|| "nil".to_owned(), tostring_num);
            sk.info_message = Some(format!("{c} Sacred Wisps using {new_name}"));
        }
    }
    nm(&nq.list, "Damage", ModType::More, Value::Num(less_damage), "Used by Sacred Wisps");
    let chance = chance.unwrap_or_else(|| panic!("{CM}:312: attempt to perform arithmetic on local 'wispsCastChance' (a nil value)"));
    nm(
        &nq.list,
        "Speed",
        ModType::More,
        Value::Num(chance - 100.0),
        "Sacred Wisps cast chance",
    );
    nm(&nq.list, "HasNoCost", ModType::Flag, Value::Bool(true), "Used by Sacred Wisps");
    mirage_skill_part(new_env, &mut mirage);
    st_mut(env).skill_mut(m).mirage = Some(mirage);
    Ok(())
}

/// `band(flags, mask) == mask` on `skill.skillCfg.flags`.
fn cfg_has_flags(sk: &ActiveSkill, mask: u32) -> bool {
    let f = sk
        .skill_cfg
        .as_ref()
        .unwrap_or_else(|| panic!("{CM}:125: attempt to index field 'skillCfg' (a nil value)"))
        .flags;
    (f as i64 as u32) & mask == mask
}

/// A cached entry's `CritChance` / `TotalDPS` / `Speed` and `ActiveSkill`: `None` = no entry; a run that stopped at a boundary is an `Err`.
fn cached_entry(env: &Env, uuid: &str, line: u32) -> Result<Option<CachedScalars>, String> {
    let sess = session(env)?;
    let cache = sess.cache();
    let Some(c) = cache.get(CalcMode::of(env), uuid) else {
        return Ok(None);
    };
    if !c.offence_ran {
        return Err(format!(
            "{CM}:{line} GlobalCache.cachedData[{}][{uuid}]: the cached run stopped at a boundary of the port",
            CalcMode::of(env).as_str()
        ));
    }
    Ok(Some((c.crit_chance, c.total_dps, c.speed)))
}

/// `GlobalCache.cachedData[env.mode][uuid].ActiveSkill`.
fn cached_skill(env: &Env, uuid: &str) -> Result<Src, String> {
    let sess = session(env)?;
    sess.cached_active_skill(CalcMode::of(env), uuid)
        .map(Src::from)
        .ok_or_else(|| format!("{CM}: GlobalCache.cachedData[..][{uuid}] vanished"))
}

/// :124-142 The Saviour `compareFunc(skill, env, config, mirageSkill)`.
fn saviour_compare(env: &Env, m: usize, j: usize, mirage_skill: Option<Src>, best: &mut Option<f64>) -> Result<Option<Src>, String> {
    let sk = &st(env).active_skill_list[j];
    let mask = mod_flag("Sword") | mod_flag("Weapon1H");
    if j == m
        || !has_type(sk, "Attack")
        || has_type(sk, "Totem")
        || has_type(sk, "SummonsTotem")
        || !cfg_has_flags(sk, mask)
        || used_by_mirage(sk)
    {
        return Ok(mirage_skill);
    }
    let uuid = cache_skill_uuid(env, j);
    let mode = CalcMode::of(env);
    let sess = session(env)?;
    // :127-129
    if !sess.cache().contains(mode, &uuid) {
        sess.build_active_skill(env, mode, j, Some(&uuid), &[])
            .map_err(|e| format!("{CM}:128 calcs.buildActiveSkill: {}", e.0))?;
    }
    // :131-139
    let Some((crit, total, _)) = cached_entry(env, &uuid, 131)? else {
        return Ok(mirage_skill);
    };
    if crit.is_some_and(|c| c > 0.0) {
        if mirage_skill.is_none() {
            *best = total;
            return cached_skill(env, &uuid).map(Some);
        }
        let t = total.unwrap_or_else(|| panic!("{CM}:135: attempt to compare nil with number (TotalDPS)"));
        let b = best.unwrap_or_else(|| panic!("{CM}:135: attempt to compare number with nil (usedSkillBestDps)"));
        if t > b {
            *best = total;
            return cached_skill(env, &uuid).map(Some);
        }
    }
    Ok(mirage_skill)
}

/// :143-153 The Saviour `preCalcFunc`.
fn saviour_pre(env: &Env, new_env: &Env, m: usize, max_warriors: &mut f64) {
    let more_damage = q_of(env, m).base(Some(&cfg_of(env, m)), &["SaviourMirageWarriorLessDamage"]);
    let nq = new_q(new_env);
    nm(&nq.list, "Damage", ModType::More, Value::Num(more_damage), "The Saviour");
    {
        let p = env.player.borrow();
        if let (Some(w1), Some(w2)) = (p.item_list.get("Weapon 1"), p.item_list.get("Weapon 2"))
            && w1.name == w2.name
        {
            *max_warriors /= 2.0;
        }
    }
    nm(
        &nq.list,
        "QuantityMultiplier",
        ModType::Base,
        Value::Num(*max_warriors),
        "The Saviour Mirage Warriors",
    );
    nm(&nq.list, "HasNoCost", ModType::Flag, Value::Bool(true), "Used by mirage");
}

/// :181-198 Tawhoa's Chosen `compareFunc`.
fn tawhoa_compare(env: &Env, m: usize, j: usize, mirage_skill: Option<Src>, t: &mut Tawhoa) -> Result<Option<Src>, String> {
    let sk = &st(env).active_skill_list[j];
    let disabled = sk.flag("disable");
    let type_match = (has_type(sk, "Slam") || has_type(sk, "Melee")) && has_type(sk, "Attack");
    let excludes = has_type(sk, "Vaal") || has_type(sk, "Totem") || has_type(sk, "SummonsTotem");
    if j == m || crate::triggers::is_triggered(env, j) || disabled || !type_match || excludes || used_by_mirage(sk) {
        return Ok(mirage_skill);
    }
    let uuid = cache_skill_uuid(env, j);
    let mode = CalcMode::of(env);
    let sess = session(env)?;
    // :187-189
    if !sess.cache().contains(mode, &uuid) || mode == CalcMode::Calculator {
        sess.build_active_skill(env, mode, j, Some(&uuid), &[])
            .map_err(|e| format!("{CM}:188 calcs.buildActiveSkill: {}", e.0))?;
    }
    // :191-195
    let entry = cached_entry(env, &uuid, 191)?;
    let take = match (&mirage_skill, &entry) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(_), Some((_, total, _))) => {
            let x = total.unwrap_or_else(|| panic!("{CM}:191: attempt to compare nil with number (TotalDPS)"));
            let b = t
                .best
                .unwrap_or_else(|| panic!("{CM}:191: attempt to compare number with nil (usedSkillBestDps)"));
            x > b
        }
    };
    if take {
        let (_, total, speed) = entry.unwrap_or_else(|| panic!("{CM}:192: attempt to index field '?' (a nil value)"));
        t.best = total;
        t.effective_source_rate = speed;
        return cached_skill(env, &uuid).map(Some);
    }
    Ok(mirage_skill)
}

/// :199-230 Tawhoa's Chosen `preCalcFunc`.
fn tawhoa_pre(env: &Env, new_env: &mut Env, m: usize, t: &mut Tawhoa) {
    let (nq, ncfg) = (new_q(new_env), new_cfg(new_env));
    let tick = misc("ServerTickRate");
    let icdr = nq.calc_mod(Some(&ncfg), &["CooldownRecovery"]);
    // :202-208
    let triggered_cd = sd_or0(&new_skill(new_env).skill, "cooldown", 202);
    let triggered_rounded = ((triggered_cd / icdr) * tick).ceil() / tick;
    let trigger_cd = sd_or0(st(env).skill(m), "cooldown", 206);
    let trigger_rounded = ((trigger_cd / icdr) * tick).ceil() / tick;
    // :210-215
    let action_cooldown = max2(triggered_rounded, trigger_rounded);
    t.trigger_rate_cap = if action_cooldown == 0.0 {
        f64::INFINITY
    } else {
        1.0 / action_cooldown
    };
    // :217 `EffectiveSourceRate ~= 0 and calcMultiSpellRotationImpact(...) or 0`
    t.skill_trigger_rate = if t.effective_source_rate == Some(0.0) {
        0.0
    } else {
        let source_rate = t
            .effective_source_rate
            .unwrap_or_else(|| panic!("Modules/CalcTriggers.lua:94: attempt to perform arithmetic on local 'sourceRate' (a nil value)"));
        let main_uuid = cache_skill_uuid(env, m);
        let mut s = crate::triggers::SimSkill::default();
        s.uuid = main_uuid.clone();
        s.cd = Some(triggered_cd);
        s.icdr = Some(icdr);
        let mut rotation = [s];
        crate::triggers::calc_multi_spell_rotation_impact(&main_uuid, &mut rotation, source_rate, Some(trigger_cd), None).0
    };
    // :220-222
    {
        let d = new_skill_mut(new_env);
        d.skill.skill_data.insert("triggerRate".into(), Value::Num(t.skill_trigger_rate));
        d.skill.skill_data.insert("triggered".into(), Value::Bool(true));
        d.skill.skill_flags.insert("triggered".into(), true);
    }
    // :225-229
    nm(&nq.list, "HasNoCost", ModType::Flag, Value::Bool(true), "Used by Tawhoa's Chosen");
    let more_damage = q_of(env, m).base(Some(&cfg_of(env, m)), &["ChieftainMirageChieftainMoreDamage"]);
    nm(&nq.list, "Damage", ModType::More, Value::Num(more_damage), "Tawhoa's Chosen");
}

/// :346-425 General's Cry: the main skill is changed in place, then `calculateMirage(env, nil)` returns nil (offence runs).
fn generals_cry(env: &mut Env, m: usize) -> Result<bool, String> {
    // :350-355
    let uuid = cache_skill_uuid(env, m);
    if is_limited(env, m) {
        return Ok(false);
    }
    // :357-359
    {
        let sk = st_mut(env).skill_mut(m);
        sk.skill_types.insert(crate::offence::skill_type_u32("Triggered"));
        sk.skill_cfg
            .as_mut()
            .unwrap_or_else(|| panic!("{CM}:358: attempt to index field 'skillCfg' (a nil value)"))
            .skill_cond
            .insert("usedByMirage".into(), true);
        sk.skill_types.insert(crate::offence::skill_type_u32("OtherThingUsesSkill"));
    }
    // :361-364
    let mode = CalcMode::of(env);
    let sess = session(env)?;
    if !sess.cache().contains(mode, &uuid) || mode == CalcMode::Calculator {
        sess.build_active_skill(env, mode, m, Some(&uuid), std::slice::from_ref(&uuid))
            .map_err(|e| format!("{CM}:362 calcs.buildActiveSkill: {}", e.0))?;
    }
    let (hit_time, time) = {
        let cache = sess.cache();
        let c = cache
            .get(mode, &uuid)
            .unwrap_or_else(|| panic!("{CM}:364: attempt to index field '?' (a nil value)"));
        let r = c.run.borrow();
        if !r.offence_ran {
            return Err(format!(
                "{CM}:364 GlobalCache.cachedData[{}][{uuid}].Env.player.output: the cached run stopped at a boundary of the port",
                mode.as_str()
            ));
        }
        let o = &r.state.player.output;
        (o.opt_num("HitTime"), o.opt_num("Time"))
    };
    // :367-373 the General's Cry gem in the main skill's slot
    let slot_of = |env: &Env, i: usize| -> Option<String> {
        let s = st(env);
        let g = s
            .skill(i)
            .socket_group
            .unwrap_or_else(|| panic!("{CM}:368: attempt to index field 'socketGroup' (a nil value)"));
        s.tab.socket_group_list()[g].slot.clone()
    };
    let main_slot = slot_of(env, m);
    let mut cooldown = 1.0;
    let mut gc = None;
    for j in 0..st(env).active_skill_list.len() {
        if name_of_skill(st(env), &st(env).active_skill_list[j]) == "General's Cry" && slot_of(env, j) == main_slot {
            let sk = &st(env).active_skill_list[j];
            cooldown = crate::offence::helpers::calc_skill_cooldown(&q_of(env, j), &cfg_of(env, j), &sk.skill_data).0;
            gc = Some(j);
            break;
        }
    }
    let gc = gc.unwrap_or_else(|| panic!("{CM}:376: attempt to index local 'generalsCryActiveSkill' (a nil value)"));
    let main_q = q_of(env, m);
    // :376-380 scale DPS with the mirage quantity
    let mut max_warriors = 0.0;
    for e in q_of(env, gc).tabulate(Some(ModType::Base), Some(&cfg_of(env, gc)), &["GeneralsCryDoubleMaxCount"]) {
        let md = &e.mod_;
        main_q.new_mod(
            "QuantityMultiplier",
            md.ty,
            md.value.clone(),
            md.source.as_deref(),
            md.flags,
            md.keyword_flags,
            Vec::new(),
        );
        max_warriors += md
            .value
            .as_num()
            .unwrap_or_else(|| panic!("{CM}:379: attempt to perform arithmetic on field 'value'"));
    }
    // :383-391
    let mut spawn_time = 0.3 + 0.2 * max_warriors;
    if has_type(st(env).skill(m), "Channel") {
        spawn_time += 1.0;
    } else {
        st_mut(env)
            .skill_mut(m)
            .skill_data
            .insert("hitTimeOverride".into(), Value::Num(1.0));
    }
    spawn_time = round(spawn_time, Some(2.0));
    // :395-396
    let t = hit_time
        .or(time)
        .unwrap_or_else(|| panic!("{CM}:395: bad argument #2 to 'max' (number expected, got nil)"));
    cooldown = max2(cooldown, t);
    main_q.new_mod(
        "DPS",
        Some(ModType::More),
        Value::Num((1.0 / cooldown - 1.0) * 100.0),
        Some("General's Cry Cooldown"),
        0,
        0,
        Vec::new(),
    );
    // :399
    main_q.new_mod(
        "HasNoCost",
        Some(ModType::Flag),
        Value::Bool(true),
        Some("Used by mirage"),
        0,
        0,
        Vec::new(),
    );
    // :402-421 supported attacks count as exerted (each Tabulate is taken before its own NewMods, as upstream's `ipairs` over the result)
    let main_cfg = cfg_of(env, m);
    for (ty, stat, target) in [
        (ModType::Inc, "ExertIncrease", "Damage"),
        (ModType::More, "ExertIncrease", "Damage"),
        (ModType::More, "ExertAttackIncrease", "Damage"),
        (ModType::More, "OverexertionExertAverageIncrease", "Damage"),
        (ModType::Base, "ExertDoubleDamageChance", "DoubleDamageChance"),
    ] {
        for e in main_q.tabulate(Some(ty), Some(&main_cfg), &[stat]) {
            let md = &e.mod_;
            main_q.new_mod(
                target,
                md.ty,
                md.value.clone(),
                md.source.as_deref(),
                md.flags,
                md.keyword_flags,
                Vec::new(),
            );
        }
    }
    // :423-424
    let name = name_of_skill(st(env), st(env).skill(m));
    let sk = st_mut(env).skill_mut(m);
    sk.info_message = Some(format!("{} GC Mirages using {name}", tostring_num(max_warriors)));
    sk.info_message2 = Some(format!(
        "{}s for {} Mirages to finish Attacking",
        tostring_num(spawn_time),
        tostring_num(max_warriors)
    ));
    // :427 `calculateMirage(env, nil)` -> nil
    Ok(false)
}
