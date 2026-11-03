//! R7.2 -- repeated runs of one build: `GlobalCache` (Data/Global.lua:356), `cacheSkillUUID` / `cacheData` / `wipeGlobalCache`
//! (Common.lua:897/:924/:944), `calcs.buildActiveSkill` (Calcs.lua:357-382), the frame of `calcs.calcFullDPS` (:142-356) and of the
//! `calcs.buildOutput` tail (:384-470), `getCachedOutputValue` (CalcPerform.lua:31-43). Line numbers: `grep -n` on upstream `ed354c2f8`,
//! 2026-10-31.
//!
//! Ownership (R7-ARCH §1 p. 1): a [`CalcSession`] holds what upstream keeps on the `build` object between runs -- the build model and
//! the parsed items (`build.itemsTab.items`) -- plus the process-global state a run leaves behind: the cache and the writes into the
//! shared `data.skills` tables ([`SkillsCarry`]; the warcry tag cell already lives in the thread-local `pob_data::skills()`). Every
//! `initEnv` makes a fresh [`Env`] (upstream does too), except `calcFullDPS`'s re-inits, which reuse its one env (R7.2b). R7-perf: the passive spec (after `PostLoad`) and the loaded
//! `build.skillsTab` are made once per session ([`BuildShared`]): every env shares the spec, and each initEnv merges the granted
//! skills into its own copy of the loaded tab -- which is what upstream's shared tab holds after the first MAIN initEnv (see
//! [`CalcSession::init_env`]).
//!
//! `cacheData` stores the WHOLE env (`Env = env`), and upstream keeps writing into the top env after caching it (the buildOutput
//! tail): the run is moved once into an `Rc<RefCell<CalcRun>>` ([`RunRef`]) shared by the cache and the caller -- no copies. An env
//! points back at its session with a `Weak` (the cache owns envs).
//!
//! :3797-3801 (`calcs.triggers`, `calcs.mirages`, `calcs.offence`) runs to its end unless it hits a boundary of the port (a mirage's copy
//! with a minion, R7.10): every cache entry says whether it did ([`CalcRun::offence_ran`]), and every reader of an offence value through the
//! cache stops at a run without it with an `unimplemented` note -- no nil is ever read as a value. R7.9: a mirage's perform (CalcMirages.lua:52)
//! is one more nested perform (cached); The Saviour / Tawhoa's Chosen leave a copy outside `activeSkillList` as `mainSkill` (cached as it is).

use std::cell::{Cell, Ref, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::BuildModel;
use pob_data::gems::gems;
use pob_items::Item;

use crate::calculate::{CalcError, CalculateOptions};
use crate::env::CalcOverride;
use crate::env::Env;
use pob_build::{ConfigModLists, ConfigState};

use crate::init_env::{BuildShared, SpecDbs, init_env_shared_mode, parse_items, reinit_accelerated};
use crate::output::{FullDpsSkill, OutTable};
use crate::perform::{ActorState, Checkpoint, PerformState, finish_output_spec, perform_skip_ehp};

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
    /// What the last MAIN initEnv merged into `build.skillsTab` (CalcSetup.lua:1468, MAIN only): a CALCULATOR / CALCS initEnv rebuilds the tab and merges these,
    /// not its own: an `override` that takes an item off (`repSlotName` / `repItem`) leaves its socket group in upstream's shared table.
    pub main_merge: Option<Rc<MainMerge>>,
}

/// `env.grantedSkills` and `env.explodeSources` of the last MAIN initEnv.
#[derive(Debug)]
pub struct MainMerge {
    pub granted: Vec<crate::env::GrantedSkill>,
    pub extra: Vec<crate::skills::GrantedSkillExtra>,
    pub explode_sources: Vec<crate::skills_tab::ExplodeSource>,
}

/// One finished `calcs.perform`: the env and what perform made (`env.player.output` & co. live in [`PerformState`]).
pub struct CalcRun {
    pub env: Env,
    pub state: PerformState,
    /// `calcs.offence` ran for `env.player.mainSkill` ([`PerformState::offence_ran`]): false where CalcPerform.lua:3797-3798 took an
    /// unported triggers / mirages branch.
    pub offence_ran: bool,
}

pub type RunRef = Rc<RefCell<CalcRun>>;

/// `GlobalCache.cachedData[mode][uuid]` (Common.lua:925-940). The scalar fields are copies taken at `cacheData` time; `run` is
/// the env itself (`Env = env`), `active_skill` is `ActiveSkill = env.player.mainSkill` as an index into
/// `run.env.skills.active_skill_list` -- or [`crate::skills::DETACHED`] (R7.9: a mirage's copy outside the list), whose skill object is
/// `detached` (taken at `cacheData`: nothing writes to a copy after its perform; the env's own `detached_main` may be replaced later).
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
    /// `ActiveSkill` when it is a detached skill (`active_skill == DETACHED`).
    pub detached: Option<Rc<crate::skills::DetachedSkill>>,
    /// [`CalcRun::offence_ran`] at `cacheData`: the scalar copies above are upstream's (false: the run stopped at a boundary of the port).
    pub offence_ran: bool,
}

/// What `GlobalCache.cachedData[mode][uuid].ActiveSkill` is, for `calcs.mirages`' readers (CalcMirages.lua:134/:137/:194), which copy it.
#[derive(Clone)]
pub(crate) enum CachedActiveSkill {
    /// `activeSkillList[i + 1]` of the cached run's env.
    Listed(RunRef, usize),
    /// `activeSkillList[i + 1]` of the env in hand: the cached run is the env being performed in place (R7.2b, `calcFullDPS`) -- upstream
    /// reads the very skill table of the env it is performing.
    Current(usize),
    /// A detached skill (R7.9).
    Detached(Rc<crate::skills::DetachedSkill>),
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
    /// `build.itemsTab.items`, parsed once (ItemsTab:Load); moved into [`SessionCore::shared`] by the first initEnv.
    items: RefCell<Option<IndexMap<u64, Rc<Item>>>>,
    /// `build.spec` + the loaded `build.skillsTab`, made by the first initEnv (R7-perf).
    shared: RefCell<Option<Rc<BuildShared>>>,
    cache: RefCell<GlobalCache>,
    carry: RefCell<SkillsCarry>,
    log: RefCell<SessionLog>,
    /// `calcs.perform` frames open now.
    depth: Cell<usize>,
    /// A MAIN initEnv has run (it is what fills the shared `build.skillsTab`, CalcSetup.lua:1452).
    main_init_done: Cell<bool>,
    /// The checkpoint hook every perform of the session runs with ([`CalcSession::set_hook`]); taken out while a perform runs, so a
    /// perform nested inside it runs without one.
    hook: RefCell<Option<SessionHook>>,
    /// The run [`CalcSession::perform_in_place`] performs now and the frame depth it was called at (R7.2b mine, R7.9): its env is mutably
    /// borrowed for the whole perform, and a reader of `cachedData[..].ActiveSkill` inside that perform gets the skill of the env in hand.
    in_place: RefCell<Option<(std::rc::Weak<RefCell<CalcRun>>, usize)>>,
    /// The `override` of the [`MiscCalculator::calc`] running now (R8.7, see [`CalcSession::init_env`]).
    ambient_override: RefCell<Option<Rc<CalcOverride>>>,
    /// `build.calcsTab.mainEnv.multipliersUsed` (Calcs.lua:457-584) of the last `build_output(MAIN)` -- what the CALCS tail reads (R8.6).
    main_multipliers_used: RefCell<Option<std::collections::HashSet<String>>>,
}

/// A checkpoint hook owned by the session (R7.3: the offence checkpoints of nested runs, e.g. Dual Savior's first nested env).
pub type SessionHook = Box<dyn FnMut(Checkpoint, &Env, &ActorState, &ActorState)>;

/// One build's calculator: `calcs.*` entry points over a shared [`GlobalCache`]. Cheap to clone (a handle).
#[derive(Clone)]
pub struct CalcSession {
    core: Rc<SessionCore>,
}

/// `calcs.calcFullDPS`'s result table (Calcs.lua:146-160, `cullingDPS` :350).
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
    /// `fullDPS.cullingDPS` -- set only when `cullingMulti > 0` (:349-352).
    pub culling_dps: Option<f64>,
    /// `fullDPS.skills`, in insertion order.
    pub skills: Vec<FullDpsSkill>,
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
    let sk = st.skill(skill);
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
                items: RefCell::new(Some(items)),
                shared: RefCell::new(None),
                cache: RefCell::new(GlobalCache::default()),
                carry: RefCell::new(SkillsCarry::default()),
                log: RefCell::new(SessionLog::default()),
                depth: Cell::new(0),
                main_init_done: Cell::new(false),
                hook: RefCell::new(None),
                in_place: RefCell::new(None),
                ambient_override: RefCell::new(None),
                main_multipliers_used: RefCell::new(None),
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

    /// Every `calcs.perform` the session runs from now on calls `hook` at the [`Checkpoint`]s (the outermost one only while
    /// performs nest). `None` removes it.
    pub fn set_hook(&self, hook: Option<SessionHook>) {
        *self.core.hook.borrow_mut() = hook;
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

    /// `calcs.initEnv(build, mode)` (CalcSetup.lua:475) -- a fresh env every call, as upstream (no `override`, no `specEnv`: see
    /// [`CalcSession::init_env_spec`]). The data writes of earlier runs are carried in ([`SkillsCarry`]) and the new ones carried out.
    ///
    /// Modes: MAIN is the ported path. CALCULATOR differs from MAIN in initEnv only by `env.buildBreakdown = false` (:500) and by
    /// branches that write display state onto shared tables and are read by no calc module (`node.finalModList` /
    /// `jewelRadiusData` :272-298, `superseded` :435/:445, `gemInstance.displayEffect`/`supportEffect` :1671-1696/:1786, the main
    /// skill write-back :1887, `displayLabel`/`displaySkillList` :1895-1913) -- plus :1452, the granted-skill merge into
    /// `build.skillsTab`, MAIN only: the shared tab a CALCULATOR initEnv reads holds what the last MAIN merge left, whatever the `override`
    /// did to the items: the tab is rebuilt and merged with the inputs of that MAIN merge ([`SkillsCarry::main_merge`]). So CALCULATOR = the MAIN path with
    /// `buildBreakdown = false`, once a MAIN initEnv has run in the session. CALCS (R8.6) is the same path with its own branches --
    /// `misc_buffMode` (CalcSetup.lua:565-583), `skill_number` (:1585), `mainActiveSkillCalcs` (:1882), the `*Calcs` gem fields
    /// (CalcActiveSkill.lua:250/623/649/694/736/924) -- and `buildBreakdown = true`; `displaySkillListCalcs` (:1912) is display state.
    pub fn init_env(&self, mode: CalcMode) -> Result<Env, SessionError> {
        // R8.7: inside [`MiscCalculator::calc`] every env descends from the calculator's env, whose nested initEnvs pass `env.override`
        // on (CalcActiveSkill.lua:182 `copyActiveSkill`, reached from `crate::mirages`) -- the calculator's override is the ambient one.
        let ambient = self.core.ambient_override.borrow().clone();
        self.init_env_override(mode, None, ambient.as_ref()).map(|(env, _)| env)
    }

    /// [`CalcSession::init_env`] with upstream's `specEnv` (R7.2b): `calcs.initEnv(build, mode, override, { cachedPlayerDB = ..,
    /// cachedEnemyDB = .., cachedMinionDB = .., env = nil })` (CalcSetup.lua:475-482) -- the base and config mods are read through the
    /// cached DBs as `parent`s (:688-693). Returns the env and the DBs initEnv returns (:1974): `spec`'s, or `specCopy` of the new env.
    pub fn init_env_spec(&self, mode: CalcMode, spec: Option<&SpecDbs>) -> Result<(Env, SpecDbs), SessionError> {
        self.init_env_override(mode, spec, None)
    }

    /// [`CalcSession::init_env_spec`] with an `override` (R8.7): `calcs.initEnv(build, mode, override, specEnv)`.
    pub fn init_env_override(
        &self,
        mode: CalcMode,
        spec: Option<&SpecDbs>,
        ov: Option<&Rc<CalcOverride>>,
    ) -> Result<(Env, SpecDbs), SessionError> {
        match mode {
            CalcMode::Main => {}
            CalcMode::Calculator if self.core.main_init_done.get() => {}
            CalcMode::Calculator => {
                return Err(SessionError::Unimplemented(
                    "CalcSetup.lua:1452 initEnv(\"CALCULATOR\") before any MAIN initEnv: the shared build.skillsTab without the granted-skill merge"
                        .into(),
                ));
            }
            CalcMode::Calcs if self.core.main_init_done.get() => {}
            CalcMode::Calcs => {
                return Err(SessionError::Unimplemented(
                    "CalcSetup.lua:1452 initEnv(\"CALCS\") before any MAIN initEnv: the shared build.skillsTab without the granted-skill merge"
                        .into(),
                ));
            }
        }
        self.core.log.borrow_mut().init_env_calls += 1;
        let t0 = std::time::Instant::now();
        let shared = self.shared().map_err(|e| SessionError::Calc(CalcError(format!("initEnv: {e}"))))?;
        let carry = self.core.carry.borrow().clone();
        let (mut env, cached) = init_env_shared_mode(
            self.core.build.clone(),
            &shared,
            self.core.opts.parse_mode,
            Some(&carry),
            spec,
            ov,
            mode.as_str(),
        )
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
        if mode == CalcMode::Main {
            self.core.carry.borrow_mut().main_merge = Some(Rc::new(MainMerge {
                granted: env.granted_skills.clone(),
                extra: env.granted_skills_extra.clone(),
                explode_sources: env.explode_sources.clone(),
            }));
        }
        // :499-500
        env.mode = mode.as_str();
        env.build_breakdown = matches!(mode, CalcMode::Main | CalcMode::Calcs);
        env.session = Rc::downgrade(&self.core);
        if mode == CalcMode::Main {
            self.core.main_init_done.set(true);
        }
        self.core.log.borrow_mut().init_env_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        Ok((env, cached))
    }

    /// What the last MAIN initEnv merged ([`SkillsCarry::main_merge`]).
    pub fn main_merge(&self) -> Option<Rc<MainMerge>> {
        self.core.carry.borrow().main_merge.clone()
    }

    /// The merge another initEnv of this session sees: an edit of the items does not reach upstream's shared `socketGroupList` until the next MAIN.
    pub fn set_main_merge(&self, merge: Option<Rc<MainMerge>>) {
        self.core.carry.borrow_mut().main_merge = merge;
    }

    /// The session's [`BuildShared`], made on first use (a load error is returned, and retried by the next initEnv).
    pub fn shared(&self) -> Result<Rc<BuildShared>, crate::init_env::InitEnvError> {
        if let Some(s) = self.core.shared.borrow().as_ref() {
            return Ok(s.clone());
        }
        let items = self
            .core
            .items
            .borrow()
            .clone()
            .expect("the items are moved into BuildShared only once it exists");
        let mut s = BuildShared::new(&self.core.build, items, self.core.opts.parse_mode)?;
        if let Some(keys) = &self.core.opts.live_alloc_pairs {
            let mut spec = (*s.spec).clone();
            spec.live_alloc_pairs = Some(keys.clone());
            s.spec = Rc::new(spec);
        }
        let s = Rc::new(s);
        *self.core.items.borrow_mut() = None;
        *self.core.shared.borrow_mut() = Some(s.clone());
        Ok(s)
    }

    /// `build.configTab` after a `BuildModList` of the ConfigTab: the state and lists the next initEnv of this session reads (CalcSetup.lua:508-509, :693-694). `GlobalCache` stays: the tab's
    /// comparison tooltips run `calcFunc` over the same cache as the calculator's base pass (ConfigTab.lua:776-779). A session that never calls this is as before.
    pub fn set_config(&self, config: ConfigState, lists: ConfigModLists) -> Result<(), CalcError> {
        self.shared().map_err(|e| CalcError(format!("initEnv: {e}")))?;
        let mut shared = self.core.shared.borrow_mut();
        let s = shared
            .as_mut()
            .and_then(Rc::get_mut)
            .ok_or_else(|| CalcError("set_config: a run holds the shared build".into()))?;
        s.config = config;
        s.config_lists = lists;
        Ok(())
    }

    /// `calcs.perform(env, skipEHP)` (CalcPerform.lua:1286-3990) -- the env moves into the returned run, which the cache shares
    /// (:3989 `cacheData(cacheSkillUUID(env.player.mainSkill, env), env)` ends EVERY perform, nested ones included).
    pub fn perform(&self, env: Env, skip_ehp: bool) -> RunRef {
        self.perform_inner(env, skip_ehp, false)
    }

    fn perform_inner(&self, env: Env, skip_ehp: bool, top: bool) -> RunRef {
        self.perform_run(env, skip_ehp, top, None)
    }

    /// `calcs.perform(newEnv)` from inside a perform that holds a checkpoint hook (CalcMirages.lua:52, R7.9): the nested perform runs with
    /// that same hook (the session's own hook is taken by the outermost perform), then `cacheData` as every perform.
    pub(crate) fn perform_hooked(&self, env: Env, skip_ehp: bool, hook: crate::offence::Hook<'_, '_>) -> RunRef {
        self.perform_run(env, skip_ehp, false, Some(hook))
    }

    fn perform_run(&self, mut env: Env, skip_ehp: bool, top: bool, hook: Option<crate::offence::Hook<'_, '_>>) -> RunRef {
        let state = self.perform_env(&mut env, skip_ehp, top, hook);
        let offence_ran = state.offence_ran;
        let run = Rc::new(RefCell::new(CalcRun { env, state, offence_ran }));
        self.cache_data(&run, top);
        run
    }

    /// `calcs.perform(env, skipEHP)` again on the env a cached run holds (R7.2b: `calcFullDPS` performs its one env once per skill,
    /// Calcs.lua:172-174, re-initialising it in place in between). The run's output is replaced -- upstream's `env.player.output = { }`
    /// (CalcPerform.lua:1305), seen by every cache entry that holds this env -- then `cacheData`. The run stays mutably borrowed for
    /// the whole perform; the session records it ([`SessionCore::in_place`]): `calcs.mirages`' read of `cachedData[..].ActiveSkill` of this
    /// very run (CalcMirages.lua:134/:137/:194 -- upstream: the env being performed) gets the skill of the env in hand
    /// ([`CalcSession::cached_active_skill`]); any other read of this run through the cache panics instead of reading a stale run.
    fn perform_in_place(&self, run: &RunRef, skip_ehp: bool) {
        let prev = self.core.in_place.replace(Some((Rc::downgrade(run), self.core.depth.get())));
        {
            let mut guard = run.borrow_mut();
            let r = &mut *guard;
            let state = self.perform_env(&mut r.env, skip_ehp, false, None);
            r.offence_ran = state.offence_ran;
            r.state = state;
        }
        *self.core.in_place.borrow_mut() = prev;
        self.cache_data(run, false);
    }

    /// `GlobalCache.cachedData[mode][uuid].ActiveSkill` for a reader that copies the skill (`calcs.copyActiveSkill`, R7.9) -- `None` = no
    /// entry. A run being performed in place ([`CalcSession::perform_in_place`]) resolves to the env in hand when the reader is that very
    /// perform (one frame below it); any other reader of a mutably borrowed run is a Lua-impossible state here and panics.
    pub(crate) fn cached_active_skill(&self, mode: CalcMode, uuid: &str) -> Option<CachedActiveSkill> {
        let cache = self.core.cache.borrow();
        let c = cache.get(mode, uuid)?;
        if let Some(d) = &c.detached {
            return Some(CachedActiveSkill::Detached(d.clone()));
        }
        if c.run.try_borrow().is_ok() {
            return Some(CachedActiveSkill::Listed(c.run.clone(), c.active_skill));
        }
        let in_place = self.core.in_place.borrow();
        match in_place.as_ref() {
            Some((w, d)) if w.upgrade().is_some_and(|r| Rc::ptr_eq(&r, &c.run)) && self.core.depth.get() == d + 1 => {
                Some(CachedActiveSkill::Current(c.active_skill))
            }
            _ => panic!(
                "GlobalCache.cachedData[{}][{uuid}].ActiveSkill: the cached run is being performed and is not the env in hand (R7.2b)",
                mode.as_str()
            ),
        }
    }

    fn perform_env(&self, env: &mut Env, skip_ehp: bool, top: bool, explicit_hook: Option<crate::offence::Hook<'_, '_>>) -> PerformState {
        let depth = self.core.depth.get();
        let mode = CalcMode::of(env);
        let at = {
            let mut log = self.core.log.borrow_mut();
            log.perform.push(PerformEntry {
                depth,
                mode,
                skip_ehp,
                top,
                uuid: cache_skill_uuid(env, main_skill_of(env)),
                ms: 0.0,
            });
            log.perform.len() - 1
        };
        let t0 = std::time::Instant::now();
        self.core.depth.set(depth + 1);
        let state = match explicit_hook {
            Some(h) => {
                let h = h.map(|f| f as &mut dyn FnMut(Checkpoint, &Env, &ActorState, &ActorState));
                perform_skip_ehp(env, self.core.opts.defence, self.core.opts.estimations, h, skip_ehp)
            }
            None => {
                let mut hook = self.core.hook.borrow_mut().take();
                let state = match hook.as_mut() {
                    Some(h) => perform_skip_ehp(env, self.core.opts.defence, self.core.opts.estimations, Some(&mut **h), skip_ehp),
                    None => perform_skip_ehp(env, self.core.opts.defence, self.core.opts.estimations, None, skip_ehp),
                };
                if hook.is_some() && self.core.hook.borrow().is_none() {
                    *self.core.hook.borrow_mut() = hook;
                }
                state
            }
        };
        self.core.depth.set(depth);
        self.core.log.borrow_mut().perform[at].ms = t0.elapsed().as_secs_f64() * 1000.0;
        state
    }

    /// `cacheData(uuid, env)` (Common.lua:924-941) with `uuid = cacheSkillUUID(env.player.mainSkill, env)`.
    fn cache_data(&self, run: &RunRef, top: bool) {
        let entry = {
            let r = run.borrow();
            let ms = main_skill_of(&r.env);
            let st = r.env.skills.as_ref().expect("skills on");
            let name = st.skills.effects[st.skill(ms).active_effect.borrow().granted_effect].name.clone();
            // R7.9: a detached main skill (a mirage's copy) is kept as the object it is now
            let detached = (ms == crate::skills::DETACHED).then(|| Rc::new(st.detached_main.clone().expect("detached main skill")));
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
                    detached,
                    offence_ran: r.offence_ran,
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
        // :358 `calcs.initEnv(env.build, mode, env.override)`
        let mut full_env = match self.init_env_override(mode, None, env.calc_override.as_ref()).map(|(e, _)| e) {
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

    /// `calcs.calcFullDPS(build, mode, {}, specEnv)` (Calcs.lua:142-356): initEnv, every enabled skill of an `includeInFullDPS` group as
    /// `mainSkill` + `perform(fullEnv, true)` and the roll-up of its output (:176-336), a new initEnv after each (upstream: the
    /// accelerated `wipeEnv` re-init of the same env, :337-344), then the
    /// ailment re-adds and the caps (:297-353).
    ///
    /// Boundaries (the roll-up stops, [`FullDpsOutcome::Unimplemented`]): a skill whose run hit a boundary of the port at :3797-3801, or whose
    /// minion's triggers / offence (CalcPerform.lua:3807-3808) did. The minion (`activeSkill.minion or usedEnv.minion`, :178-212, R7.10b) and
    /// `activeSkill.mirage` (:214-251, R7.9) are rolled up.
    ///
    /// Hash-order (R13/L10): none -- `fullDPS.skills` is an array, the ailment sources are plain locals compared with a strict `>` (the
    /// first of equal values wins, as upstream).
    #[allow(clippy::too_many_lines)]
    pub fn calc_full_dps(&self, mode: CalcMode) -> Result<FullDpsOutcome, CalcError> {
        self.calc_full_dps_spec(mode, None)
    }

    /// `calcs.calcFullDPS(build, mode, {}, specEnv)` with upstream's `specEnv` (R7.2b): `spec` = the DBs the caller's initEnv returned
    /// (Calcs.lua:392 / :93 / :107), `None` = `specEnv` nil. The first initEnv reads the base / config mods through them (:143); after each
    /// skill the SAME env is re-initialised in place with every accelerator on (:337-344, [`reinit_accelerated`]) and performed again --
    /// so every cache entry this makes holds that one env, as upstream's `cacheData` (`Env = env`).
    pub fn calc_full_dps_spec(&self, mode: CalcMode, spec: Option<SpecDbs>) -> Result<FullDpsOutcome, CalcError> {
        self.calc_full_dps_override(mode, spec, None)
    }

    /// [`CalcSession::calc_full_dps_spec`] with an `override` (Calcs.lua:107 `calcs.calcFullDPS(build, "CALCULATOR", override, specEnv)`).
    #[allow(clippy::too_many_lines)]
    pub fn calc_full_dps_override(
        &self,
        mode: CalcMode,
        spec: Option<SpecDbs>,
        ov: Option<&Rc<CalcOverride>>,
    ) -> Result<FullDpsOutcome, CalcError> {
        let (full_env, cached) = match self.init_env_override(mode, spec.as_ref(), ov) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => {
                return Ok(FullDpsOutcome::Unimplemented {
                    processed: 0,
                    note: format!("Calcs.lua:143 {what}"),
                });
            }
        };
        let mut fd = FullDps::default();
        // :162-166 the ailment sources
        let mut bleed_source = String::new();
        let mut corrupting_blood_source = String::new();
        let mut ignite_source = String::new();
        let mut burning_ground_source = String::new();
        let mut caustic_ground_source = String::new();
        let mut processed = 0;
        // :168 `ipairs(fullEnv.player.activeSkillList)` -- the list of the first env; the accelerated re-init keeps it
        // (`accelerate.skills`, CalcSetup.lua:372).
        let n = full_env.skills.as_ref().map_or(0, |s| s.active_skill_list.len());
        // The env's own notes from initEnv: a re-init drops the ones its last perform added (a fresh env would not hold them).
        let init_notes = full_env.unimplemented.len();
        // `fullEnv` before its first perform; from then on it lives in `run` (the cache holds it too).
        let mut fresh = Some(full_env);
        let mut run: Option<RunRef> = None;
        for i in 0..n {
            let guard = run.as_ref().map(|r| r.borrow());
            let full_env: &Env = match (&fresh, &guard) {
                (Some(e), _) => e,
                (None, Some(r)) => &r.env,
                (None, None) => unreachable!("the env is either fresh or in the run"),
            };
            let included = {
                let st = full_env.skills.as_ref().expect("skills on");
                st.active_skill_list[i]
                    .socket_group
                    .is_some_and(|g| st.tab.socket_group_list()[g].include_in_full_dps == Some(true))
            };
            if !included {
                continue;
            }
            let (count, enabled) = get_active_skill_count(full_env, i);
            drop(guard);
            if !enabled {
                continue;
            }
            // :172-174
            let run = match fresh.take() {
                Some(mut env) => {
                    env.skills.as_mut().expect("skills on").main_skill = Some(i);
                    let r = self.perform_inner(env, true, false);
                    run = Some(r.clone());
                    r
                }
                None => {
                    let r = run.clone().expect("the env is in the run");
                    r.borrow_mut().env.skills.as_mut().expect("skills on").main_skill = Some(i);
                    self.perform_in_place(&r, true);
                    r
                }
            };
            processed += 1;
            let mut count = count;
            {
                // :207-211 `activeSkillCount = 1`, `activeSkill.infoMessage2 = "Skill Damage"` (the last statement of the minion block: nothing
                // in it reads either, so it is applied first -- the `infoMessage2` write needs the run mutably)
                let mut r = run.borrow_mut();
                let has_minion = r.env.skills.as_ref().expect("skills on").active_skill_list[i].minion.is_some() || r.env.minion.is_some();
                if has_minion {
                    let name = {
                        let st = r.env.skills.as_ref().expect("skills on");
                        st.skills.effects[st.active_skill_list[i].active_effect.borrow().granted_effect]
                            .name
                            .clone()
                    };
                    let db = r.env.mod_db.clone();
                    let once =
                        |pat: &str, cond: &str| name.contains(pat) && crate::setup::ok(db.borrow().flag(None, &[cond])) == Some(true);
                    if once("Absolution", "Condition:AbsolutionSkillDamageCountedOnce")
                        || once("Dominating Blow", "Condition:DominatingBlowSkillDamageCountedOnce")
                        || once("Holy Strike", "Condition:HolyStrikeSkillDamageCountedOnce")
                    {
                        count = 1.0;
                        r.env.skills.as_mut().expect("skills on").active_skill_list[i].info_message2 = Some("Skill Damage".to_owned());
                    }
                }
            }
            {
                let r = run.borrow();
                let st = r.env.skills.as_ref().expect("skills on");
                let sk = &st.active_skill_list[i];
                let name = st.skills.effects[sk.active_effect.borrow().granted_effect].name.clone();
                // :176-212 `if activeSkill.minion or usedEnv.minion` (R7.10b)
                let mut minion_name: Option<String> = None;
                if sk.minion.is_some() || r.env.minion.is_some() {
                    let Some(ms) = r.state.minion.as_ref().filter(|_| r.state.minion_offence_ran) else {
                        let why = r
                            .env
                            .unimplemented
                            .iter()
                            .find(|u| u.starts_with("CalcTriggers.lua:"))
                            .cloned()
                            .unwrap_or_else(|| "calcs.offence(env, env.minion) did not run".to_owned());
                        return Ok(FullDpsOutcome::Unimplemented {
                            processed,
                            note: format!("Calcs.lua:178-212 calcFullDPS roll-up of the minion skill {name}: {why}"),
                        });
                    };
                    let ms = ms.borrow();
                    let mo = &ms.output;
                    let mnum = |k: &str, line: u32| {
                        mo.opt_num(k).unwrap_or_else(|| {
                            panic!("Modules/Calcs.lua:{line}: attempt to perform arithmetic on field '{k}' (a nil value)")
                        })
                    };
                    let mgt = |k: &str, best: f64| mo.opt_num(k).is_some_and(|v| v > best);
                    if mgt("TotalDPS", 0.0) {
                        // :180 `(activeSkill.minion and activeSkill.minion.minionData.name..": ") or (usedEnv.minion ...) or ""`
                        let mname = sk.minion.as_deref().map(|m| m.md("name")).unwrap_or(pob_mods::Value::Nil);
                        let mname = match mname {
                            pob_mods::Value::Str(s) => format!("{s}: "),
                            other => panic!(
                                "Modules/Calcs.lua:180: attempt to concatenate field 'name' (a {} value)",
                                other.type_name()
                            ),
                        };
                        let part = sk
                            .skill_part_name
                            .clone()
                            .unwrap_or_else(|| panic!("Modules/Calcs.lua:181: attempt to concatenate field 'skillPartName' (a nil value)"));
                        let dps = mnum("TotalDPS", 181);
                        fd.skills.push(FullDpsSkill {
                            name: name.clone(),
                            dps,
                            count,
                            trigger: sk.info_trigger.clone(),
                            skill_part: Some(format!("{mname}{part}")),
                            source: None,
                        });
                        fd.combined_dps += dps * count;
                        minion_name = Some(mname);
                    }
                    if mgt("BleedDPS", fd.bleed_dps) {
                        fd.bleed_dps = mnum("BleedDPS", 185);
                        bleed_source.clone_from(&name);
                    }
                    if mgt("IgniteDPS", fd.ignite_dps) {
                        fd.ignite_dps = mnum("IgniteDPS", 189);
                        ignite_source.clone_from(&name);
                    }
                    if mgt("PoisonDPS", 0.0) {
                        fd.total_poison_dps += mnum("TotalPoisonDPS", 193) * count;
                    }
                    if mgt("ImpaleDPS", 0.0) {
                        fd.impale_dps += mnum("ImpaleDPS", 196) * count;
                    }
                    if mgt("DecayDPS", 0.0) {
                        fd.decay_dps += mnum("DecayDPS", 199);
                    }
                    if mgt("TotalDot", 0.0) {
                        fd.dot_dps += mnum("TotalDot", 202);
                    }
                    if mo.opt_num("CullMultiplier").is_some_and(|v| v > 1.0 && v > fd.culling_multi) {
                        fd.culling_multi = mnum("CullMultiplier", 205);
                    }
                }
                // :214-251 `activeSkill.mirage` and :254 `activeSkill.infoTrigger` are set by calcs.mirages / calcs.triggers; a run
                // whose dispatch took an unported branch has no offence either.
                if !r.offence_ran {
                    let why = r
                        .env
                        .unimplemented
                        .iter()
                        .find(|u| u.starts_with("CalcTriggers.lua:") || u.starts_with("CalcMirages.lua:"))
                        .cloned()
                        .unwrap_or_else(|| "calcs.offence did not run".to_owned());
                    return Ok(FullDpsOutcome::Unimplemented {
                        processed,
                        note: format!("Calcs.lua:176-336 calcFullDPS roll-up of {name}: {why}"),
                    });
                }
                // `activeSkill.infoTrigger`: calcs.triggers (R7.8, `ActiveSkill::info_trigger`) runs after CalcPerform.lua:1487-1490
                // (The Saviour, recorded by the skill loop under this note -- perform/skills_loop.rs), so its value wins.
                let saviour = format!("CalcPerform.lua:1488-1489 activeSkill.infoMessage/infoTrigger of skill {} (", i + 1);
                let trigger = sk.info_trigger.clone().or_else(|| {
                    r.env
                        .unimplemented
                        .iter()
                        .any(|u| u.starts_with(&saviour))
                        .then(|| "Saviour".to_owned())
                });
                let o = &r.state.player.output;
                let pos = |k: &str| o.opt_num(k).is_some_and(|v| v > 0.0);
                let num = |k: &str, line: u32| {
                    o.opt_num(k)
                        .unwrap_or_else(|| panic!("Modules/Calcs.lua:{line}: attempt to perform arithmetic on field '{k}' (a nil value)"))
                };
                // :214-251 `activeSkill.mirage` (R7.9: Mirage Archer / Sacred Wisps, CalcMirages.lua:76/:302)
                if let Some(m) = &sk.mirage {
                    let mirage_count = m.count.unwrap_or(1.0) * count;
                    let mnum = |k: &str, line: u32| {
                        m.out(k).unwrap_or_else(|| {
                            panic!("Modules/Calcs.lua:{line}: attempt to perform arithmetic on field '{k}' (a nil value)")
                        })
                    };
                    let mgt = |k: &str, best: f64| m.out(k).is_some_and(|v| v > best);
                    if m.output.is_none() {
                        panic!("Modules/Calcs.lua:216: attempt to index field 'output' (a nil value)");
                    }
                    if mgt("TotalDPS", 0.0) {
                        let dps = mnum("TotalDPS", 217);
                        fd.skills.push(FullDpsSkill {
                            name: format!("{} (Mirage)", m.name),
                            dps,
                            count: mirage_count,
                            // `activeSkill.mirage.infoTrigger` -- no code writes it (nil)
                            trigger: None,
                            skill_part: m.skill_part_name.clone(),
                            source: None,
                        });
                        fd.combined_dps += dps * mirage_count;
                    }
                    if mgt("BleedDPS", fd.bleed_dps) {
                        fd.bleed_dps = mnum("BleedDPS", 221);
                        bleed_source = format!("{name} (Mirage)");
                    }
                    if mgt("IgniteDPS", fd.ignite_dps) {
                        fd.ignite_dps = mnum("IgniteDPS", 225);
                        ignite_source = format!("{name} (Mirage)");
                    }
                    if mgt("PoisonDPS", 0.0) {
                        fd.total_poison_dps += mnum("TotalPoisonDPS", 229) * mirage_count;
                    }
                    if mgt("ImpaleDPS", 0.0) {
                        fd.impale_dps += mnum("ImpaleDPS", 232) * mirage_count;
                    }
                    if mgt("DecayDPS", 0.0) {
                        fd.decay_dps += mnum("DecayDPS", 235);
                    }
                    if mgt("TotalDot", 0.0) && (sk.flag("DotCanStack") || o.opt_num("TotalDot") == Some(0.0)) {
                        let k = if sk.flag("DotCanStack") { mirage_count } else { 1.0 };
                        fd.dot_dps += mnum("TotalDot", 238) * k;
                    }
                    if m.out("CullMultiplier").is_some_and(|v| v > 1.0 && v > fd.culling_multi) {
                        fd.culling_multi = mnum("CullMultiplier", 241);
                    }
                    if mgt("BurningGroundDPS", fd.burning_ground_dps) {
                        fd.burning_ground_dps = mnum("BurningGroundDPS", 244);
                        burning_ground_source = format!("{name} (Mirage)");
                    }
                    if mgt("CausticGroundDPS", fd.caustic_ground_dps) {
                        fd.caustic_ground_dps = mnum("CausticGroundDPS", 248);
                        caustic_ground_source = format!("{name} (Mirage)");
                    }
                }
                // :253-256
                if pos("TotalDPS") {
                    let dps = num("TotalDPS", 254);
                    fd.skills.push(FullDpsSkill {
                        name: name.clone(),
                        dps,
                        count,
                        trigger,
                        // :254 `minionName and activeSkill.infoMessage2 or activeSkill.skillPartName`
                        skill_part: match minion_name {
                            Some(_) if sk.info_message2.is_some() => sk.info_message2.clone(),
                            _ => sk.skill_part_name.clone(),
                        },
                        source: None,
                    });
                    fd.combined_dps += dps * count;
                }
                // `output[k] and output[k] > best` (a non-number is the Lua compare error)
                let gt = |k: &str, best: f64| o.opt_num(k).is_some_and(|v| v > best);
                if gt("BleedDPS", fd.bleed_dps) {
                    fd.bleed_dps = num("BleedDPS", 258);
                    bleed_source.clone_from(&name);
                }
                if gt("CorruptingBloodDPS", fd.corrupting_blood_dps) {
                    fd.corrupting_blood_dps = num("CorruptingBloodDPS", 262);
                    corrupting_blood_source.clone_from(&name);
                }
                if gt("IgniteDPS", fd.ignite_dps) {
                    fd.ignite_dps = num("IgniteDPS", 266);
                    ignite_source.clone_from(&name);
                }
                if gt("BurningGroundDPS", fd.burning_ground_dps) {
                    fd.burning_ground_dps = num("BurningGroundDPS", 270);
                    burning_ground_source.clone_from(&name);
                }
                if pos("PoisonDPS") {
                    fd.total_poison_dps += num("TotalPoisonDPS", 274) * count;
                }
                if gt("CausticGroundDPS", fd.caustic_ground_dps) {
                    fd.caustic_ground_dps = num("CausticGroundDPS", 277);
                    caustic_ground_source.clone_from(&name);
                }
                if pos("ImpaleDPS") {
                    fd.impale_dps += num("ImpaleDPS", 281) * count;
                }
                if pos("DecayDPS") {
                    fd.decay_dps += num("DecayDPS", 284);
                }
                if pos("TotalDot") {
                    let stack = if sk.flag("DotCanStack") { count } else { 1.0 };
                    fd.dot_dps += num("TotalDot", 287) * stack;
                }
                if o.opt_num("CullMultiplier").is_some_and(|v| v > 1.0 && v > fd.culling_multi) {
                    fd.culling_multi = num("CullMultiplier", 290);
                }
            }
            // :337-344 `calcs.initEnv(build, mode, override, { cached DBs, env = fullEnv, accelerate = everything })`
            {
                let t0 = std::time::Instant::now();
                let mut r = run.borrow_mut();
                r.env.unimplemented.truncate(init_notes);
                reinit_accelerated(&mut r.env, &cached);
                let mut log = self.core.log.borrow_mut();
                log.init_env_calls += 1;
                log.init_env_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
        // :297-353 re-add the ailment DPS components
        let cap = crate::setup::misc("DotDpsCap");
        let entry = |name: &str, dps: f64, source: Option<&String>| FullDpsSkill {
            name: name.to_owned(),
            dps,
            count: 1.0,
            trigger: None,
            skill_part: None,
            source: source.cloned(),
        };
        fd.total_dot_dps = 0.0;
        if fd.bleed_dps > 0.0 {
            fd.skills.push(entry("Best Bleed DPS", fd.bleed_dps, Some(&bleed_source)));
            fd.total_dot_dps += fd.bleed_dps;
        }
        if fd.corrupting_blood_dps > 0.0 {
            fd.skills.push(entry(
                "Corrupting Blood DPS",
                fd.corrupting_blood_dps,
                Some(&corrupting_blood_source),
            ));
            fd.total_dot_dps += fd.corrupting_blood_dps;
        }
        if fd.ignite_dps > 0.0 {
            fd.skills.push(entry("Best Ignite DPS", fd.ignite_dps, Some(&ignite_source)));
            fd.total_dot_dps += fd.ignite_dps;
        }
        if fd.burning_ground_dps > 0.0 {
            fd.skills.push(entry(
                "Best Burning Ground DPS",
                fd.burning_ground_dps,
                Some(&burning_ground_source),
            ));
            fd.total_dot_dps += fd.burning_ground_dps;
        }
        if fd.total_poison_dps > 0.0 {
            fd.total_poison_dps = pob_lua::number::min2(fd.total_poison_dps, cap);
            fd.skills.push(entry("Full Poison DPS", fd.total_poison_dps, None));
            fd.total_dot_dps += fd.total_poison_dps;
        }
        if fd.caustic_ground_dps > 0.0 {
            fd.skills.push(entry(
                "Best Caustic Ground DPS",
                fd.caustic_ground_dps,
                Some(&caustic_ground_source),
            ));
            fd.total_dot_dps += fd.caustic_ground_dps;
        }
        if fd.impale_dps > 0.0 {
            fd.skills.push(entry("Full Impale DPS", fd.impale_dps, None));
            fd.combined_dps += fd.impale_dps;
        }
        if fd.decay_dps > 0.0 {
            fd.skills.push(entry("Full Decay DPS", fd.decay_dps, None));
            fd.total_dot_dps += fd.decay_dps;
        }
        if fd.dot_dps > 0.0 {
            fd.skills.push(entry("Full DoT DPS", fd.dot_dps, None));
            fd.total_dot_dps += fd.dot_dps;
        }
        fd.total_dot_dps = pob_lua::number::min2(fd.total_dot_dps, cap);
        fd.combined_dps += fd.total_dot_dps;
        if fd.culling_multi > 0.0 {
            let c = fd.combined_dps * (fd.culling_multi - 1.0);
            fd.culling_dps = Some(c);
            fd.skills.push(entry("Full Culling DPS", c, None));
            fd.combined_dps += c;
        }
        Ok(FullDpsOutcome::Done(fd))
    }

    /// `calcs.getMiscCalculator(build)` (Calcs.lua:89-115), its base pass (:91-98): `initEnv(build, "CALCULATOR")`, perform, then
    /// `calcFullDPS(build, "CALCULATOR", {}, specEnv)` with the DBs that initEnv returned (:93), whose result goes into the output when
    /// it has skills (`usedFullDPS`, :94-98). The calculator it returns: [`CalcSession::get_misc_calculator`].
    pub fn misc_calculator_base(&self) -> Result<BuildOutput, CalcError> {
        let (env, cached) = match self.init_env_spec(CalcMode::Calculator, None) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => return Err(CalcError(format!("Calcs.lua:91 {what}"))),
        };
        // :92 `calcs.perform(env)`
        let run = self.perform_inner(env, false, true);
        let full_dps = self.calc_full_dps_spec(CalcMode::Calculator, Some(cached))?;
        match &full_dps {
            // :94-98 `usedFullDPS = #fullDPS.skills > 0`
            FullDpsOutcome::Done(fd) if !fd.skills.is_empty() => {
                let mut r = run.borrow_mut();
                let o = &mut r.state.player.output;
                o.set_table("SkillDPS", OutTable::SkillDps(fd.skills.clone()));
                o.set_num("FullDPS", fd.combined_dps);
                o.set_num("FullDotDPS", fd.total_dot_dps);
            }
            FullDpsOutcome::Done(_) => {}
            FullDpsOutcome::Unimplemented { processed, note } => {
                let what = format!("Calcs.lua:93-98 calcFullDPS ({processed} skills performed): {note}");
                run.borrow_mut().env.unimplemented.push(what);
            }
        }
        Ok(BuildOutput { run, full_dps })
    }

    /// `calcs.getMiscCalculator(build)` (Calcs.lua:89-115): the base pass ([`CalcSession::misc_calculator_base`]) and the calculator
    /// closure over it ([`MiscCalculator::calc`]). Needs a MAIN initEnv first (CalcsTab:BuildOutput runs `buildOutput(MAIN)` before it).
    pub fn get_misc_calculator(&self) -> Result<MiscCalculator, CalcError> {
        let base = self.misc_calculator_base()?;
        let used_full_dps = match &base.full_dps {
            FullDpsOutcome::Done(fd) => Ok(!fd.skills.is_empty()),
            FullDpsOutcome::Unimplemented { note, .. } => Err(note.clone()),
        };
        Ok(MiscCalculator {
            session: self.clone(),
            base: base.run,
            used_full_dps,
        })
    }

    /// `calcs.buildOutput(build, mode)` (Calcs.lua:384-829): MAIN (the cost warnings, :439-449, and `multipliersUsed` of :457-584) and
    /// CALCS (R8.6, the tail :601-829: `BuffList` / `CombatList` / `CurseList`, `SkillBuffs` / `SkillDebuffs`; it reads the
    /// `multipliersUsed` of the last MAIN `build_output` -- `build.calcsTab.mainEnv`, CalcsTab.lua:458-461 runs MAIN first).
    #[allow(clippy::too_many_lines)]
    pub fn build_output(&self, mode: CalcMode) -> Result<BuildOutput, CalcError> {
        // :386-387
        let (env, cached) = match self.init_env_spec(mode, None) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => return Err(CalcError(format!("Calcs.lua:386 {what}"))),
        };
        let run = self.perform_inner(env, false, true);

        // :392 `calcs.calcFullDPS(build, "CALCULATOR", {}, ...)`
        let full_dps = self.calc_full_dps_spec(CalcMode::Calculator, Some(cached))?;
        // :395-397 `env.player.output.SkillDPS/FullDPS/FullDotDPS`
        {
            let mut r = run.borrow_mut();
            match &full_dps {
                FullDpsOutcome::Done(fd) => {
                    let o = &mut r.state.player.output;
                    o.set_table("SkillDPS", OutTable::SkillDps(fd.skills.clone()));
                    o.set_num("FullDPS", fd.combined_dps);
                    o.set_num("FullDotDPS", fd.total_dot_dps);
                }
                FullDpsOutcome::Unimplemented { processed, note } => {
                    let what = format!("Calcs.lua:392-398 calcFullDPS ({processed} skills performed): {note}");
                    r.env.unimplemented.push(what);
                }
            }
        }

        if mode == CalcMode::Main {
            // :400-437 every skill: build it if MAIN has no entry, then the cost checks against the cache
            let (uuids, names, main_name): (Vec<String>, Vec<String>, Option<String>) = {
                let r = run.borrow();
                let st = r.env.skills.as_ref().expect("skills on");
                let name_of = |i: usize| st.skills.effects[st.skill(i).active_effect.borrow().granted_effect].name.clone();
                let n = st.active_skill_list.len();
                (
                    (0..n).map(|i| cache_skill_uuid(&r.env, i)).collect(),
                    (0..n).map(name_of).collect(),
                    st.main_skill.map(name_of),
                )
            };
            for (i, uuid) in uuids.iter().enumerate() {
                if !self.core.cache.borrow().contains(mode, uuid) {
                    let r = run.borrow();
                    self.build_active_skill(&r.env, mode, i, Some(uuid), &[])?;
                }
                // the cached run's costs (read before any write: the top run may be that same run)
                let cached = {
                    let cache = self.core.cache.borrow();
                    cache.get(mode, uuid).map(|c| {
                        let cr = c.run.borrow();
                        let o = &cr.state.player.output;
                        let st = cr.env.skills.as_ref().expect("skills on");
                        let ms = st.skill(main_skill_of(&cr.env));
                        let costs: Vec<Option<f64>> = COST_POOLS
                            .iter()
                            .chain(PERCENT_COST_POOLS.iter())
                            .map(|(_, c)| o.opt_num(c))
                            .collect();
                        let reserved: Vec<pob_mods::Value> = COST_POOLS
                            .iter()
                            .map(|(pool, _)| {
                                let raw = pool.strip_suffix("Unreserved").unwrap_or(pool);
                                ms.skill_data.get(&format!("{raw}ReservedPercent")).cloned().unwrap_or_default()
                            })
                            .collect();
                        (cr.offence_ran, costs, reserved)
                    })
                };
                let Some((offence_ran, costs, reserved)) = cached else { continue };
                let mut r = run.borrow_mut();
                let CalcRun { env, state, .. } = &mut *r;
                // :406 `output.EnergyShieldProtectsMana = env.modDB:Flag(nil, "EnergyShieldProtectsMana")`
                let espm = crate::setup::ok(env.mod_db.borrow().flag(None, &["EnergyShieldProtectsMana"]));
                let o = &mut state.player.output;
                match espm {
                    Some(b) => o.set_bool("EnergyShieldProtectsMana", b),
                    None => o.remove("EnergyShieldProtectsMana"),
                }
                if !offence_ran {
                    // the cached run stopped at the triggers / mirages dispatch: its costs (offence part A) are missing
                    let what = format!(
                        "Calcs.lua:407-436 cost warnings of {}: the cached run stopped at a boundary of the port before or in calcs.offence",
                        names[i]
                    );
                    if !env.unimplemented.contains(&what) {
                        env.unimplemented.push(what);
                    }
                    continue;
                }
                let is_main = main_name.as_deref() == Some(names[i].as_str());
                // :407-427 `pairs({...})`: each pool writes only its own keys -- order-free (R13); constructor order
                let espm_on = espm == Some(true);
                for (k, (pool, cost)) in COST_POOLS.iter().enumerate() {
                    let Some(cached_cost) = costs[k] else { continue };
                    let es = if espm_on && *cost == "ManaCost" {
                        o.opt_num("EnergyShield").unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let total_pool = es + o.opt_num(pool).unwrap_or(0.0);
                    if total_pool < cached_cost {
                        let raw = pool.strip_suffix("Unreserved").unwrap_or(pool);
                        let warn = match &reserved[k] {
                            pob_mods::Value::Nil | pob_mods::Value::Bool(false) => true,
                            v => {
                                let res = v.as_num().unwrap_or_else(|| {
                                    panic!(
                                        "Modules/Calcs.lua:414: attempt to perform arithmetic on local 'reservation' ({})",
                                        v.type_name()
                                    )
                                });
                                total_pool + (o.opt_num(raw).unwrap_or(0.0) * res / 100.0).ceil() < cached_cost
                            }
                        };
                        if warn {
                            if is_main {
                                o.set_bool(&format!("{cost}Warning"), true);
                            }
                            push_name(o, &format!("{cost}WarningList"), &names[i]);
                        }
                    }
                }
                // :428-436
                for (k, (pool, cost)) in PERCENT_COST_POOLS.iter().enumerate() {
                    let Some(cached_cost) = costs[COST_POOLS.len() + k] else { continue };
                    if o.opt_num(pool).unwrap_or(0.0) < cached_cost {
                        push_name(o, &format!("{cost}PercentCostWarningList"), &names[i]);
                    }
                }
            }
            // :439-449
            let mut r = run.borrow_mut();
            let CalcRun { env, state, .. } = &mut *r;
            finish_output_spec(env, state);
            env.unimplemented
                .push("Calcs.lua:450-464 env.skillsUsed / display stats (P5)".into());
            // :457-584 -- the part of the `*Used` tables the CALCS tail reads (`mainEnv.multipliersUsed`, R8.6)
            *self.core.main_multipliers_used.borrow_mut() = Some(crate::calcs_tail::multipliers_used(env));
        } else if mode == CalcMode::Calcs {
            // :601-829
            let mult = self
                .core
                .main_multipliers_used
                .borrow()
                .clone()
                .unwrap_or_else(|| panic!("Modules/Calcs.lua:637: attempt to index field 'mainEnv' (a nil value)"));
            let mut r = run.borrow_mut();
            let CalcRun { env, state, .. } = &mut *r;
            crate::calcs_tail::calcs_tail(env, state, &mult);
        }
        Ok(BuildOutput { run, full_dps })
    }
}

/// Calcs.lua:407 `{ LifeUnreserved = "LifeCost", ManaUnreserved = "ManaCost", Rage = "RageCost", EnergyShield = "ESCost" }`.
const COST_POOLS: [(&str, &str); 4] = [
    ("LifeUnreserved", "LifeCost"),
    ("ManaUnreserved", "ManaCost"),
    ("Rage", "RageCost"),
    ("EnergyShield", "ESCost"),
];

/// Calcs.lua:428 `{ LifeUnreservedPercent = "LifePercentCost", ManaUnreservedPercent = "ManaPercentCost" }`.
const PERCENT_COST_POOLS: [(&str, &str); 2] = [
    ("LifeUnreservedPercent", "LifePercentCost"),
    ("ManaUnreservedPercent", "ManaPercentCost"),
];

/// `output[key] = output[key] or {}; t_insert(output[key], name)`.
fn push_name(o: &mut crate::output::Output, key: &str, name: &str) {
    let mut list = match o.table(key) {
        Some(OutTable::Names(v)) => v.clone(),
        _ => Vec::new(),
    };
    list.push(name.to_owned());
    o.set_table(key, OutTable::Names(list));
}

/// `getCachedOutputValue(env, activeSkill, ...)` (CalcPerform.lua:31-43): the output values of another skill's run, built (and
/// cached) on demand -- MAIN reads the cache, CALCULATOR always rebuilds. `skill` indexes `env`'s `activeSkillList`.
///
/// `Ok(values)` when the cached run reached `calcs.offence`; a run stopped at an unported triggers / mirages branch has none of the
/// offence output asked for (`Speed`, `HitSpeed`, `Duration`), so it returns `Err(note)` at that boundary -- after doing the build
/// exactly as upstream does.
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
            "CalcPerform.lua:39 getCachedOutputValue({}): the cached run stopped at a boundary of the port before or in calcs.offence",
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

/// What `calcs.getMiscCalculator(build)` returns (Calcs.lua:89-115): the closure `function(override, useFullDPS)` and the base output.
pub struct MiscCalculator {
    session: CalcSession,
    /// The base pass (:91-98) -- `env.player.output` of it is the second return value.
    pub base: RunRef,
    /// `usedFullDPS` (:94): the base `calcFullDPS` rolled up a skill; `Err` = its roll-up stopped at a boundary of the port.
    pub used_full_dps: Result<bool, String>,
}

impl MiscCalculator {
    /// The session the calculator runs in.
    pub fn session(&self) -> &CalcSession {
        &self.session
    }

    /// The closure (Calcs.lua:99-114): `initEnv(build, "CALCULATOR", override)` (no `specEnv`), perform, then -- when
    /// `(useFullDPS ~= false or build.viewMode == "TREE") and usedFullDPS` -- `calcFullDPS(build, "CALCULATOR", override, specEnv)` with the
    /// DBs that initEnv returned, its result written into the output. `use_full_dps`: `None` = nil. Returns the run (`env.player.output`).
    pub fn calc(&self, ov: &Rc<CalcOverride>, use_full_dps: Option<bool>, view_mode_tree: bool) -> Result<RunRef, CalcError> {
        // Every initEnv nested in this call sees `env.override` (Calcs.lua:358, CalcActiveSkill.lua:182): the ambient override.
        let prev = self.session.core.ambient_override.replace(Some(ov.clone()));
        let r = self.calc_inner(ov, use_full_dps, view_mode_tree);
        *self.session.core.ambient_override.borrow_mut() = prev;
        r
    }

    fn calc_inner(&self, ov: &Rc<CalcOverride>, use_full_dps: Option<bool>, view_mode_tree: bool) -> Result<RunRef, CalcError> {
        let s = &self.session;
        let (env, cached) = match s.init_env_override(CalcMode::Calculator, None, Some(ov)) {
            Ok(e) => e,
            Err(SessionError::Calc(e)) => return Err(e),
            Err(SessionError::Unimplemented(what)) => return Err(CalcError(format!("Calcs.lua:100 {what}"))),
        };
        // :101 `calcs.perform(env)`
        let run = s.perform_inner(env, false, true);
        if use_full_dps != Some(false) || view_mode_tree {
            match &self.used_full_dps {
                Ok(false) => {}
                Ok(true) => {
                    // :106-110
                    match s.calc_full_dps_override(CalcMode::Calculator, Some(cached), Some(ov))? {
                        FullDpsOutcome::Done(fd) => {
                            let mut r = run.borrow_mut();
                            let o = &mut r.state.player.output;
                            o.set_table("SkillDPS", OutTable::SkillDps(fd.skills.clone()));
                            o.set_num("FullDPS", fd.combined_dps);
                            o.set_num("FullDotDPS", fd.total_dot_dps);
                        }
                        FullDpsOutcome::Unimplemented { processed, note } => {
                            let what = format!("Calcs.lua:107-110 calcFullDPS ({processed} skills performed): {note}");
                            run.borrow_mut().env.unimplemented.push(what);
                        }
                    }
                }
                Err(note) => {
                    let what = format!("Calcs.lua:102 usedFullDPS unknown -- the base calcFullDPS stopped: {note}");
                    run.borrow_mut().env.unimplemented.push(what);
                }
            }
        }
        Ok(run)
    }
}
