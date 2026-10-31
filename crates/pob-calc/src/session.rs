//! R7.2 -- repeated runs of one build: `GlobalCache` (Data/Global.lua:356), `cacheSkillUUID` / `cacheData` / `wipeGlobalCache`
//! (Common.lua:897/:924/:944), `calcs.buildActiveSkill` (Calcs.lua:357-382), the frame of `calcs.calcFullDPS` (:142-356) and of the
//! `calcs.buildOutput` tail (:384-470), `getCachedOutputValue` (CalcPerform.lua:31-43). Line numbers: `grep -n` on upstream `ed354c2f8`,
//! 2026-10-31.
//!
//! Ownership (R7-ARCH §1 p. 1): a [`CalcSession`] holds what upstream keeps on the `build` object between runs -- the build model and
//! the parsed items (`build.itemsTab.items`) -- plus the process-global state a run leaves behind: the cache and the writes into the
//! shared `data.skills` tables ([`SkillsCarry`]; the warcry tag cell already lives in the thread-local `pob_data::skills()`). Every
//! `initEnv` makes a fresh [`Env`] (upstream does too); the passive spec and `build.skillsTab` are rebuilt from the model each time,
//! which is what upstream's shared copies hold after the first MAIN initEnv (see [`CalcSession::init_env`]).
//!
//! `cacheData` stores the WHOLE env (`Env = env`), and upstream keeps writing into the top env after caching it (the buildOutput
//! tail): the run is moved once into an `Rc<RefCell<CalcRun>>` ([`RunRef`]) shared by the cache and the caller -- no copies. An env
//! points back at its session with a `Weak` (the cache owns envs).
//!
//! Offence (`calcs.offence`, CalcPerform.lua:3797-3801) is not ported yet (R7.3-R7.7): every cache entry says so
//! ([`CalcRun::offence_ran`]), and every reader of an offence value through the cache stops there with an `unimplemented` note -- no
//! nil is ever read as a value.

use std::cell::{Cell, Ref, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::BuildModel;
use pob_data::gems::gems;
use pob_items::Item;

use crate::calculate::{CalcError, CalculateOptions};
use crate::env::Env;
use crate::init_env::{InitEnvOptions, init_env_seeded, parse_items};
use crate::perform::{PerformState, finish_output_spec, mod_db_flag, perform_skip_ehp};

/// `env.mode` -- the three keys of `GlobalCache.cachedData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalcMode {
    Main,
    Calcs,
    Calculator,
}

impl CalcMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CalcMode::Main => "MAIN",
            CalcMode::Calcs => "CALCS",
            CalcMode::Calculator => "CALCULATOR",
        }
    }

    /// The mode of an env (`env.mode`).
    pub fn of(env: &Env) -> CalcMode {
        match env.mode {
            "MAIN" => CalcMode::Main,
            "CALCS" => CalcMode::Calcs,
            "CALCULATOR" => CalcMode::Calculator,
            other => panic!("env.mode {other:?}"),
        }
    }
}

/// The writes a run leaves on the shared `data.skills` tables that the next initEnv of the process starts from (see
/// [`crate::init_env::init_env_seeded`]). Only ever grows (upstream never undoes them).
#[derive(Clone, Debug, Default)]
pub struct SkillsCarry {
    /// `grantedEffect.fromItem = true` (CalcSetup.lua:1633-1634), by effect id.
    pub from_item_marks: BTreeSet<String>,
    /// `grantedEffect.levels[level].cost = {}` (SkillsTab ProcessSocketGroup), as `(effect id, level)`.
    pub cost_wiped: Vec<(String, f64)>,
}

/// One finished `calcs.perform`: the env and what perform made (`env.player.output` & co. live in [`PerformState`]).
pub struct CalcRun {
    pub env: Env,
    pub state: PerformState,
    /// `calcs.offence` ran for `env.player.mainSkill` -- always `false` until R7.7 (CalcPerform.lua:3797-3801 is skipped).
    pub offence_ran: bool,
}

pub type RunRef = Rc<RefCell<CalcRun>>;

/// `GlobalCache.cachedData[mode][uuid]` (Common.lua:925-940). The scalar fields are copies taken at `cacheData` time; `run` is
/// the env itself (`Env = env`), `active_skill` is `ActiveSkill = env.player.mainSkill` as an index into
/// `run.env.skills.active_skill_list`.
pub struct CachedSkill {
    pub name: String,
    pub speed: Option<f64>,
    pub hit_speed: Option<f64>,
    pub mana_cost: Option<f64>,
    pub life_cost: Option<f64>,
    pub es_cost: Option<f64>,
    pub rage_cost: Option<f64>,
    pub hit_chance: Option<f64>,
    pub accuracy_hit_chance: Option<f64>,
    pub pre_effective_crit_chance: Option<f64>,
    pub crit_chance: Option<f64>,
    pub total_dps: Option<f64>,
    pub active_skill: usize,
    pub run: RunRef,
}

/// `GlobalCache` (Data/Global.lua:356-358): `{ MAIN = {}, CALCS = {}, CALCULATOR = {} }`, uuid -> entry. Upstream's tables are
/// string-keyed hashes (no reader walks them); here insertion order.
#[derive(Default)]
pub struct GlobalCache {
    main: IndexMap<String, CachedSkill>,
    calcs: IndexMap<String, CachedSkill>,
    calculator: IndexMap<String, CachedSkill>,
}

impl GlobalCache {
    fn table(&self, mode: CalcMode) -> &IndexMap<String, CachedSkill> {
        match mode {
            CalcMode::Main => &self.main,
            CalcMode::Calcs => &self.calcs,
            CalcMode::Calculator => &self.calculator,
        }
    }

    fn table_mut(&mut self, mode: CalcMode) -> &mut IndexMap<String, CachedSkill> {
        match mode {
            CalcMode::Main => &mut self.main,
            CalcMode::Calcs => &mut self.calcs,
            CalcMode::Calculator => &mut self.calculator,
        }
    }

    /// `GlobalCache.cachedData[mode][uuid]`.
    pub fn get(&self, mode: CalcMode, uuid: &str) -> Option<&CachedSkill> {
        self.table(mode).get(uuid)
    }

    pub fn contains(&self, mode: CalcMode, uuid: &str) -> bool {
        self.table(mode).contains_key(uuid)
    }

    /// The uuids of one mode, insertion order.
    pub fn uuids(&self, mode: CalcMode) -> Vec<String> {
        self.table(mode).keys().cloned().collect()
    }

    /// `wipeGlobalCache()` (Common.lua:944-948).
    pub fn wipe(&mut self) {
        self.main.clear();
        self.calcs.clear();
        self.calculator.clear();
    }
}

/// `calcs.buildActiveSkill` calls, in call order (the R7.1 probe's `buildActiveSkillLog`).
#[derive(Clone, Debug, PartialEq)]
pub struct BasEntry {
    /// `calcs.perform` frames open at the call.
    pub depth: usize,
    pub mode: CalcMode,
    pub env_mode: CalcMode,
    /// `cacheSkillUUID(skill, env)`.
    pub skill_uuid: String,
    pub target_uuid: String,
    /// `limitedProcessingFlags`.
    pub limited: Vec<String>,
}

/// `calcs.perform` calls, in call order (`performLog`); `uuid` = `cacheSkillUUID(env.player.mainSkill, env)` at entry.
#[derive(Clone, Debug, PartialEq)]
pub struct PerformEntry {
    pub depth: usize,
    pub mode: CalcMode,
    pub skip_ehp: bool,
    pub top: bool,
    pub uuid: String,
    /// Wall time of this perform (nested ones included), ms.
    pub ms: f64,
}

/// `cacheData` writes, in order (`cacheWriteLog`).
#[derive(Clone, Debug, PartialEq)]
pub struct CacheWrite {
    pub mode: CalcMode,
    pub uuid: String,
    pub top: bool,
}

/// What the session did -- the call logs the R7.1 fixtures carry (`logs.*`), plus the session's own declared gaps.
#[derive(Clone, Debug, Default)]
pub struct SessionLog {
    pub init_env_calls: usize,
    /// Wall time of each initEnv, ms.
    pub init_env_ms: Vec<f64>,
    pub bas: Vec<BasEntry>,
    pub perform: Vec<PerformEntry>,
    pub cache_writes: Vec<CacheWrite>,
    /// `ConPrintf` lines (Calcs.lua:381).
    pub con_print: Vec<String>,
    /// Upstream branches a nested run / the session skipped (the top env's own list is `Env::unimplemented`).
    pub unimplemented: Vec<String>,
    /// Wall time of the `buildActiveSkill` runs (initEnv + perform), ms, one per entry of `bas` that ran.
    pub bas_ms: Vec<f64>,
}

/// Why [`CalcSession::init_env`] made no env.
#[derive(Debug)]
pub enum SessionError {
    Calc(CalcError),
    /// An upstream branch not ported -- `"<File.lua>:<line> <what>"`.
    Unimplemented(String),
}

impl From<CalcError> for SessionError {
    fn from(e: CalcError) -> Self {
        SessionError::Calc(e)
    }
}

/// The session state shared by every env of one build (reached from an env through [`Env::session`]).
pub struct SessionCore {
    build: Rc<BuildModel>,
    opts: CalculateOptions,
    /// `build.itemsTab.items`, parsed once (ItemsTab:Load).
    items: IndexMap<u64, Rc<Item>>,
    cache: RefCell<GlobalCache>,
    carry: RefCell<SkillsCarry>,
    log: RefCell<SessionLog>,
    /// `calcs.perform` frames open now.
    depth: Cell<usize>,
    /// A MAIN initEnv has run (it is what fills the shared `build.skillsTab`, CalcSetup.lua:1452).
    main_init_done: Cell<bool>,
}

/// One build's calculator: `calcs.*` entry points over a shared [`GlobalCache`]. Cheap to clone (a handle).
#[derive(Clone)]
pub struct CalcSession {
    core: Rc<SessionCore>,
}

/// `calcs.calcFullDPS`'s result table (Calcs.lua:146-160) -- the numbers only (`skills` entries are R7.7).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FullDps {
    pub combined_dps: f64,
    pub total_dot_dps: f64,
    pub total_poison_dps: f64,
    pub caustic_ground_dps: f64,
    pub impale_dps: f64,
    pub ignite_dps: f64,
    pub burning_ground_dps: f64,
    pub bleed_dps: f64,
    pub corrupting_blood_dps: f64,
    pub decay_dps: f64,
    pub dot_dps: f64,
    pub culling_multi: f64,
    /// `#fullDPS.skills`.
    pub skills: usize,
}

/// What [`CalcSession::calc_full_dps`] could do.
#[derive(Debug)]
pub enum FullDpsOutcome {
    /// No skill was rolled up (no enabled `includeInFullDPS` group): the result is exact.
    Done(FullDps),
    /// `processed` skills ran (`perform(fullEnv, true)`), but their roll-up reads offence output -- `note` says where it stops.
    Unimplemented { processed: usize, note: String },
}

/// `calcs.buildOutput`'s result: the top run and what the session did.
pub struct BuildOutput {
    pub run: RunRef,
    pub full_dps: FullDpsOutcome,
}

/// Lua `%s` (C locale): space, `\t`, `\n`, `\v`, `\f`, `\r`.
fn strip_lua_space(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r'))
        .collect()
}

/// `cacheSkillUUID(skill, env)` (Common.lua:897-922) for `env.player.activeSkillList[skill + 1]`. Gem identity (`gem ==
/// skill.activeEffect.srcInstance`, :907-913) is the `(group, gem)` index pair the skill phase records as `srcInstance`.
pub fn cache_skill_uuid(env: &Env, skill: usize) -> String {
    let st = env.skills.as_ref().expect("cacheSkillUUID: the skill phase is off");
    let sk = &st.active_skill_list[skill];
    let effect = sk.active_effect.borrow();
    // :898 `skill.activeEffect.grantedEffect.name:gsub("%s+", "")`
    let str_name = strip_lua_space(&st.skills.effects[effect.granted_effect].name);
    let groups = st.tab.socket_group_list();
    let group = sk.socket_group.and_then(|i| groups.get(i).map(|g| (i, g)));
    // :899 `(skill.socketGroup and skill.socketGroup.slot and skill.socketGroup.slot:upper() or "NO_SLOT"):gsub("%s+", "")`
    let str_slot_name = strip_lua_space(&match group.and_then(|(_, g)| g.slot.as_deref()) {
        Some(slot) => slot.to_ascii_uppercase(),
        None => "NO_SLOT".to_owned(),
    });
    // :900-913
    let mut slot_indx = 1;
    if let (Some((gi, g)), Some(src)) = (group, effect.src_instance) {
        for idx in 0..g.gem_list.len() {
            if src == (gi, idx) {
                slot_indx = idx + 1;
                break;
            }
        }
    }
    // :915-920
    let group_idx = group.map_or(1, |(gi, _)| gi + 1);
    format!("{str_name}_{str_slot_name}_{slot_indx}_{group_idx}")
}

/// `env.player.mainSkill` as an index (the skill phase always sets one, CalcSetup.lua:1929-1938).
fn main_skill_of(env: &Env) -> usize {
    env.skills
        .as_ref()
        .and_then(|s| s.main_skill)
        .expect("env.player.mainSkill (the skill phase is off)")
}

/// `getActiveSkillCount(activeSkill)` (Calcs.lua:116-140) -> `(count, enabled)`.
fn get_active_skill_count(env: &Env, skill: usize) -> (f64, bool) {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[skill];
    let Some(gi) = sk.socket_group else {
        return (1.0, true);
    };
    let group = &st.tab.socket_group_list()[gi];
    if let Some(n) = group.group_count {
        return (n, true);
    }
    let effect_id = &st.skills.effects[sk.active_effect.borrow().granted_effect].id;
    for gi in &group.gem_list {
        let Some(gd) = gi.gem_data.as_deref() else { continue };
        let gem = &gems().gems[gd];
        let granted = Some(&gem.granted_effect_id).filter(|id| st.skills.get(id).is_some());
        let secondary = gem.secondary_granted_effect_id.as_ref().filter(|id| st.skills.get(id).is_some());
        if gem.vaal_gem == Some(true) {
            // `grantedEffectList[1]` / `[2]`
            if granted == Some(effect_id) {
                return (gi.count.unwrap_or(1.0), gi.enable_global1 == Some(true));
            } else if secondary == Some(effect_id) {
                return (gi.count.unwrap_or(1.0), gi.enable_global2 == Some(true));
            }
        } else {
            let support = granted.and_then(|id| st.skills.get(id)).is_some_and(|ge| ge.support);
            if (granted == Some(effect_id) && !support) || secondary == Some(effect_id) {
                return (gi.count.unwrap_or(1.0), true);
            }
        }
    }
    (1.0, true)
}

impl CalcSession {
    /// A session over one loaded build: the items are parsed once here (`build.itemsTab`); the cache starts empty (R7.1: the
    /// golden IS `wipeGlobalCache` + one `buildOutput(MAIN)` on 6/6 goldens).
    pub fn new(build: Rc<BuildModel>, opts: CalculateOptions) -> CalcSession {
        let items = parse_items(&build, opts.parse_mode);
        CalcSession {
            core: Rc::new(SessionCore {
                build,
                opts,
                items,
                cache: RefCell::new(GlobalCache::default()),
                carry: RefCell::new(SkillsCarry::default()),
                log: RefCell::new(SessionLog::default()),
                depth: Cell::new(0),
                main_init_done: Cell::new(false),
            }),
        }
    }

    /// The session an env was built by (`None` for an env made outside a session, or after the session is gone).
    pub fn of(env: &Env) -> Option<CalcSession> {
        env.session.upgrade().map(|core| CalcSession { core })
    }

    pub fn build(&self) -> &Rc<BuildModel> {
        &self.core.build
    }

    pub fn cache(&self) -> Ref<'_, GlobalCache> {
        self.core.cache.borrow()
    }

    pub fn log(&self) -> Ref<'_, SessionLog> {
        self.core.log.borrow()
    }

    /// `wipeGlobalCache()` (Common.lua:944-948).
    pub fn wipe_global_cache(&self) {
        self.core.cache.borrow_mut().wipe();
    }

    fn note(&self, what: String) {
        let mut log = self.core.log.borrow_mut();
        if !log.unimplemented.contains(&what) {
            log.unimplemented.push(what);
        }
    }

    /// `calcs.initEnv(build, mode)` (CalcSetup.lua:475) -- a fresh env every call, as upstream (no `override`, no `specEnv`
    /// accelerators). The data writes of earlier runs are carried in ([`SkillsCarry`]) and the new ones carried out.
    ///
    /// Modes: MAIN is the ported path. CALCULATOR differs from MAIN in initEnv only by `env.buildBreakdown = false` (:500) and by
    /// branches that write display state onto shared tables and are read by no calc module (`node.finalModList` /
    /// `jewelRadiusData` :272-298, `superseded` :435/:445, `gemInstance.displayEffect`/`supportEffect` :1671-1696/:1786, the main
    /// skill write-back :1887, `displayLabel`/`displaySkillList` :1895-1913) -- plus :1452, the granted-skill merge into
    /// `build.skillsTab`, MAIN only: the shared tab a CALCULATOR initEnv reads holds what the last MAIN merge left, which is what
    /// rebuilding the tab and merging again gives. So CALCULATOR = the MAIN path with `buildBreakdown = false`, once a MAIN initEnv has
    /// run in the session (else the merge would be one upstream does not do). CALCS (`misc_buffMode`, `*Calcs` gem fields,
    /// CalcActiveSkill.lua:250/623/649/694/736/924) is not ported.
    pub fn init_env(&self, mode: CalcMode) -> Result<Env, SessionError> {
        match mode {
            CalcMode::Main => {}
            CalcMode::Calculator if self.core.main_init_done.get() => {}
            CalcMode::Calculator => {
                return Err(SessionError::Unimplemented(
                    "CalcSetup.lua:1452 initEnv(\"CALCULATOR\") before any MAIN initEnv: the shared build.skillsTab without the granted-skill merge"
                        .into(),
                ));
            }
            CalcMode::Calcs => {
                return Err(SessionError::Unimplemented(
                    "CalcSetup.lua:566-567/1585/1882-1884 initEnv(\"CALCS\") (misc_buffMode, mainActiveSkillCalcs, *Calcs gem fields)"
                        .into(),
                ));
            }
        }
        self.core.log.borrow_mut().init_env_calls += 1;
        let t0 = std::time::Instant::now();
        let opts = InitEnvOptions {
            spec: None,
            items: Some(self.core.items.clone()),
            parse_mode: self.core.opts.parse_mode,
        };
        let carry = self.core.carry.borrow().clone();
        let mut env = init_env_seeded(self.core.build.clone(), opts, true, Some(&carry))
            .map_err(|e| SessionError::Calc(CalcError(format!("initEnv: {e}"))))?;
        if let Some(st) = &env.skills {
            let mut c = self.core.carry.borrow_mut();
            c.from_item_marks.extend(st.from_item_marks.iter().cloned());
            for k in &st.cost_wiped {
                if !c.cost_wiped.contains(k) {
                    c.cost_wiped.push(k.clone());
                }
            }
        }
        // :499-500
        env.mode = mode.as_str();
        env.build_breakdown = matches!(mode, CalcMode::Main | CalcMode::Calcs);
        env.session = Rc::downgrade(&self.core);
        if mode == CalcMode::Main {
            self.core.main_init_done.set(true);
        }
        self.core.log.borrow_mut().init_env_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        Ok(env)
    }

    /// `calcs.perform(env, skipEHP)` (CalcPerform.lua:1286-3990) -- the env moves into the returned run, which the cache shares
    /// (:3989 `cacheData(cacheSkillUUID(env.player.mainSkill, env), env)` ends EVERY perform, nested ones included).
    pub fn perform(&self, env: Env, skip_ehp: bool) -> RunRef {
        self.perform_inner(env, skip_ehp, false)
    }

    fn perform_inner(&self, mut env: Env, skip_ehp: bool, top: bool) -> RunRef {
        let depth = self.core.depth.get();
        let mode = CalcMode::of(&env);
        let at = {
            let mut log = self.core.log.borrow_mut();
            log.perform.push(PerformEntry {
                depth,
                mode,
                skip_ehp,
                top,
                uuid: cache_skill_uuid(&env, main_skill_of(&env)),
                ms: 0.0,
            });
            log.perform.len() - 1
        };
        let t0 = std::time::Instant::now();
        self.core.depth.set(depth + 1);
        let state = perform_skip_ehp(&mut env, self.core.opts.defence, self.core.opts.estimations, None, skip_ehp);
        self.core.depth.set(depth);
        self.core.log.borrow_mut().perform[at].ms = t0.elapsed().as_secs_f64() * 1000.0;
        let run = Rc::new(RefCell::new(CalcRun {
            env,
            state,
            offence_ran: false,
        }));
        self.cache_data(&run, top);
        run
    }

    /// `cacheData(uuid, env)` (Common.lua:924-941) with `uuid = cacheSkillUUID(env.player.mainSkill, env)`.
    fn cache_data(&self, run: &RunRef, top: bool) {
        let entry = {
            let r = run.borrow();
            let ms = main_skill_of(&r.env);
            let st = r.env.skills.as_ref().expect("skills on");
            let name = st.skills.effects[st.active_skill_list[ms].active_effect.borrow().granted_effect]
                .name
                .clone();
            let o = &r.state.player.output;
            (
                CalcMode::of(&r.env),
                cache_skill_uuid(&r.env, ms),
                CachedSkill {
                    name,
                    speed: o.opt_num("Speed"),
                    hit_speed: o.opt_num("HitSpeed"),
                    mana_cost: o.opt_num("ManaCost"),
                    life_cost: o.opt_num("LifeCost"),
                    es_cost: o.opt_num("ESCost"),
                    rage_cost: o.opt_num("RageCost"),
                    hit_chance: o.opt_num("HitChance"),
                    accuracy_hit_chance: o.opt_num("AccuracyHitChance"),
                    pre_effective_crit_chance: o.opt_num("PreEffectiveCritChance"),
                    crit_chance: o.opt_num("CritChance"),
                    total_dps: o.opt_num("TotalDPS"),
                    active_skill: ms,
                    run: run.clone(),
                },
            )
        };
        let (mode, uuid, cached) = entry;
        self.core.log.borrow_mut().cache_writes.push(CacheWrite {
            mode,
            uuid: uuid.clone(),
            top,
        });
        self.core.cache.borrow_mut().table_mut(mode).insert(uuid, cached);
    }

    /// `calcs.buildActiveSkill(env, mode, skill, targetUUID, limitedProcessingFlags)` (Calcs.lua:357-382): a new initEnv of the
    /// same build, `buildBreakdown = false`, `limitedSkills` = the caller's plus `limited`, `mainSkill` = the skill whose
    /// `cacheSkillUUID` is `target_uuid`, `perform(fullEnv, true)` (which caches it). `skill` indexes `env`'s `activeSkillList`.
    pub fn build_active_skill(
        &self,
        env: &Env,
        mode: CalcMode,
        skill: usize,
        target_uuid: Option<&str>,
        limited: &[String],
    ) -> Result<(), CalcError> {
        let skill_uuid = cache_skill_uuid(env, skill);
        let target = target_uuid.map_or_else(|| skill_uuid.clone(), str::to_owned);
        self.core.log.borrow_mut().bas.push(BasEntry {
            depth: self.core.depth.get(),
            mode,
            env_mode: CalcMode::of(env),
            skill_uuid,
            target_uuid: target.clone(),
            limited: limited.to_vec(),
        });
        let t0 = std::time::Instant::now();
        // :358 `calcs.initEnv(env.build, mode, env.override)` -- `override` is never set on these goldens' paths (no override port).
        let mut full_env = match self.init_env(mode) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => {
                self.note(format!("Calcs.lua:358 buildActiveSkill: {what}"));
                return Ok(());
            }
        };
        // :359
        full_env.build_breakdown = false;
        // :363-370 `fullEnv.limitedSkills = fullEnv.limitedSkills or {}`, then the caller's, then the flags
        let mut lim = full_env.limited_skills.take().unwrap_or_default();
        if let Some(parent) = &env.limited_skills {
            lim.extend(parent.iter().cloned());
        }
        lim.extend(limited.iter().cloned());
        full_env.limited_skills = Some(lim);
        // :373-380
        let n = full_env.skills.as_ref().map_or(0, |s| s.active_skill_list.len());
        for i in 0..n {
            if cache_skill_uuid(&full_env, i) == target {
                full_env.skills.as_mut().expect("skills on").main_skill = Some(i);
                self.perform_inner(full_env, true, false);
                self.core.log.borrow_mut().bas_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                return Ok(());
            }
        }
        // :381
        let st = env.skills.as_ref().expect("skills on");
        let name = &st.skills.effects[st.active_skill_list[skill].active_effect.borrow().granted_effect].name;
        self.core
            .log
            .borrow_mut()
            .con_print
            .push(format!("[calcs.buildActiveSkill] Failed to process skill: {name}"));
        Ok(())
    }

    /// `calcs.calcFullDPS(build, mode, {}, specEnv)` (Calcs.lua:142-356). The frame: initEnv, every enabled skill of an
    /// `includeInFullDPS` group as `mainSkill` + `perform(fullEnv, true)`, a new initEnv after each (upstream: the accelerated
    /// `wipeEnv` re-init, :337-344 -- here a fresh one: the accelerators are upstream's shortcut to the same env), then the ailment
    /// re-adds. The per-skill roll-up (:176-336) reads offence output (`TotalDPS`, `BleedDPS`, ..., minions, mirages): R7.7.
    pub fn calc_full_dps(&self, mode: CalcMode) -> Result<FullDpsOutcome, CalcError> {
        let mut full_env = match self.init_env(mode) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => {
                return Ok(FullDpsOutcome::Unimplemented {
                    processed: 0,
                    note: format!("Calcs.lua:143 {what}"),
                });
            }
        };
        let mut processed = 0;
        // :168 `ipairs(fullEnv.player.activeSkillList)` -- the list of the first env; the accelerated re-init keeps it
        // (`accelerate.skills`, CalcSetup.lua:372), a fresh one rebuilds the same list.
        let n = full_env.skills.as_ref().map_or(0, |s| s.active_skill_list.len());
        for i in 0..n {
            let included = {
                let st = full_env.skills.as_ref().expect("skills on");
                st.active_skill_list[i]
                    .socket_group
                    .is_some_and(|g| st.tab.socket_group_list()[g].include_in_full_dps == Some(true))
            };
            if !included {
                continue;
            }
            let (_count, enabled) = get_active_skill_count(&full_env, i);
            if !enabled {
                continue;
            }
            // :172-174
            full_env.skills.as_mut().expect("skills on").main_skill = Some(i);
            self.perform_inner(full_env, true, false);
            processed += 1;
            // :176-336 the roll-up of usedEnv's output -- offence (R7.7).
            // :337-344 re-init
            full_env = match self.init_env(mode) {
                Ok(e) => e,
                Err(SessionError::Calc(e)) => return Err(e),
                Err(SessionError::Unimplemented(what)) => {
                    return Ok(FullDpsOutcome::Unimplemented {
                        processed,
                        note: format!("Calcs.lua:344 {what}"),
                    });
                }
            };
        }
        if processed > 0 {
            return Ok(FullDpsOutcome::Unimplemented {
                processed,
                note:
                    "Calcs.lua:176-336 calcFullDPS roll-up of the skills' TotalDPS/ailment/minion/mirage output (needs calcs.offence, R7.7)"
                        .into(),
            });
        }
        // :297-355 with nothing rolled up: every component is 0, so no entry is added; the caps keep 0.
        let mut fd = FullDps::default();
        fd.total_dot_dps = pob_lua::number::min2(fd.total_dot_dps, crate::setup::misc("DotDpsCap"));
        fd.combined_dps += fd.total_dot_dps;
        Ok(FullDpsOutcome::Done(fd))
    }

    /// `calcs.buildOutput(build, mode)` (Calcs.lua:384-470) up to the display tables. Only MAIN is ported (the caller's mode).
    pub fn build_output(&self, mode: CalcMode) -> Result<BuildOutput, CalcError> {
        // :386-387
        let env = match self.init_env(mode) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => return Err(CalcError(format!("Calcs.lua:386 {what}"))),
        };
        let run = self.perform_inner(env, false, true);

        // :392 `calcs.calcFullDPS(build, "CALCULATOR", {}, ...)`
        let full_dps = self.calc_full_dps(CalcMode::Calculator)?;
        // :395-397 `env.player.output.SkillDPS/FullDPS/FullDotDPS` -- the keys are claimed with the roll-up (R7.7), even when it is
        // exact (nothing rolled up): R7-ARCH §3, R7.2 card p. 3.
        {
            let what = match &full_dps {
                FullDpsOutcome::Done(fd) => format!(
                    "Calcs.lua:395-397 SkillDPS/FullDPS/FullDotDPS not written (calcFullDPS ran, 0 skills rolled up: combinedDPS {}, TotalDotDPS {}; claimed in R7.7)",
                    pob_lua::tostring_num(fd.combined_dps),
                    pob_lua::tostring_num(fd.total_dot_dps)
                ),
                FullDpsOutcome::Unimplemented { processed, note } => {
                    format!("Calcs.lua:392-398 calcFullDPS ({processed} skills performed): {note}")
                }
            };
            run.borrow_mut().env.unimplemented.push(what);
        }

        if mode == CalcMode::Main {
            // :400-437 every skill: build it if MAIN has no entry, then the cost checks against the cache
            let uuids: Vec<String> = {
                let r = run.borrow();
                let n = r.env.skills.as_ref().map_or(0, |s| s.active_skill_list.len());
                (0..n).map(|i| cache_skill_uuid(&r.env, i)).collect()
            };
            let mut cost_note = false;
            for (i, uuid) in uuids.iter().enumerate() {
                if !self.core.cache.borrow().contains(mode, uuid) {
                    let r = run.borrow();
                    self.build_active_skill(&r.env, mode, i, Some(uuid), &[])?;
                }
                let cached_offence = self.core.cache.borrow().get(mode, uuid).map(|c| c.run.borrow().offence_ran);
                if let Some(offence_ran) = cached_offence {
                    // :406 `output.EnergyShieldProtectsMana = env.modDB:Flag(nil, "EnergyShieldProtectsMana")` (true or nil)
                    let mut r = run.borrow_mut();
                    let espm = mod_db_flag(&r.env, "EnergyShieldProtectsMana");
                    if espm {
                        r.state.player.output.set_bool("EnergyShieldProtectsMana", true);
                    } else {
                        r.state.player.output.remove("EnergyShieldProtectsMana");
                    }
                    // :407-436 `cachedCost = ...Env.player.output[costResource]` -- the costs are offence output.
                    if !offence_ran && !cost_note {
                        cost_note = true;
                        r.env.unimplemented.push(
                            "Calcs.lua:407-436 cost warnings: the cached LifeCost/ManaCost/RageCost/ESCost/*PercentCost come from calcs.offence (R7.7)"
                                .into(),
                        );
                    }
                }
            }
            // :439-449
            let mut r = run.borrow_mut();
            let CalcRun { env, state, .. } = &mut *r;
            finish_output_spec(env, state);
            env.unimplemented
                .push("Calcs.lua:450-464 env.skillsUsed / display stats (P5)".into());
        }
        Ok(BuildOutput { run, full_dps })
    }
}

/// `getCachedOutputValue(env, activeSkill, ...)` (CalcPerform.lua:31-43): the output values of another skill's run, built (and
/// cached) on demand -- MAIN reads the cache, CALCULATOR always rebuilds. `skill` indexes `env`'s `activeSkillList`.
///
/// `Ok(values)` once offence runs; until then (R7.7) every value it is asked for is offence output (`Speed`, `HitSpeed`,
/// `Duration`), so it returns `Err(note)` at that boundary -- after doing the build exactly as upstream does.
pub fn get_cached_output_value(env: &Env, skill: usize, keys: &[&str]) -> Result<Vec<Option<f64>>, String> {
    let session = CalcSession::of(env).ok_or_else(|| "CalcPerform.lua:31 getCachedOutputValue outside a CalcSession".to_owned())?;
    let mode = CalcMode::of(env);
    // :32
    let uuid = cache_skill_uuid(env, skill);
    // :33-35
    if !session.cache().contains(mode, &uuid) || mode == CalcMode::Calculator {
        session
            .build_active_skill(env, mode, skill, Some(&uuid), std::slice::from_ref(&uuid))
            .map_err(|e| e.0)?;
    }
    // :37-41 `GlobalCache.cachedData[env.mode][uuid].Env.player.output[v]` (a missing entry is the Lua error)
    let cache = session.cache();
    let entry = cache
        .get(mode, &uuid)
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:39: attempt to index field '?' (a nil value)"));
    let run = entry.run.borrow();
    if !run.offence_ran {
        return Err(format!(
            "CalcPerform.lua:39 getCachedOutputValue({}): the cached run's output has no calcs.offence yet (R7.7)",
            keys.join(", ")
        ));
    }
    Ok(keys.iter().map(|k| run.state.player.output.opt_num(k)).collect())
}

/// `env.limitedSkills[cacheSkillUUID(skill, env)]` (CalcPerform.lua:2279, CalcTriggers.lua:1417/:1603, CalcMirages.lua:353).
pub fn is_limited(env: &Env, skill: usize) -> bool {
    env.limited_skills
        .as_ref()
        .is_some_and(|l| l.contains(&cache_skill_uuid(env, skill)))
}
