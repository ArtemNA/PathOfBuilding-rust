//! The one entry point of the engine -- build XML in, `stats`/`meta` out -- the desktop's load path (Build.lua:640-697:
//! sections loaded, `calcsTab:BuildOutput()` -> `calcs.buildOutput(build, "MAIN")`, then `RefreshStatList()` ->
//! `EstimatePlayerProgress()`), pob-ts `src/api/engine.ts`. Pure by construction (W12): every call builds its own model,
//! spec, config state and env; the only shared thing is the immutable `data/` snapshot.
//!
//! Two input paths (MINEFIELD-RUST R7): [`calculate_file`] reads the file the way desktop PoB does
//! (`pob_build::read_build_file`, text mode CRLF -> LF); [`calculate`] / [`calculate_with`] take the text exactly as given --
//! "raw bytes", what the Linux-docker fixtures saw (the CLI applies `desktop_text_mode` to stdin itself).

use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use indexmap::IndexMap;
use pob_build::ConfigModLists;

use crate::env::Env;
use crate::init_env::{InitEnvOptions, init_env_with};
use crate::output::{StatValue, output_to_stats};
use crate::perform::{PerformState, PhaseFn, finish_output, perform};
use crate::session::{CalcMode, CalcRun, CalcSession, SessionLog};
use crate::{EnvMeta, meta_of};

/// What `calculate` plugs into perform's `calcs.defence` / `calcs.buildDefenceEstimations` call sites (CalcPerform.lua:
/// 3792-3795). `None` records the call site in `unimplemented`. The default is the real `defence` (R4.3) and the real
/// `buildDefenceEstimations` (R4.4b).
#[derive(Clone, Copy)]
pub struct CalculateOptions {
    pub defence: Option<PhaseFn>,
    pub estimations: Option<PhaseFn>,
    /// How mod lines are read (R5.4 decision 1): `Full` = the desktop (ModCache, else the real ModParser) -- the default;
    /// `CacheOnly` = pob-ts (the R3/R4 "Rust = pob-ts" gates).
    pub parse_mode: pob_parser::ParseMode,
    /// The skill phase of initEnv (R6.3, R6-ARCH §2 p. 5): `true` (default) = CalcSetup.lua's skills; `false` = the pre-R6 path (the
    /// "= pob-ts" gates pin it).
    pub skills: bool,
}

impl Default for CalculateOptions {
    fn default() -> CalculateOptions {
        CalculateOptions {
            defence: Some(crate::defence::defence),
            estimations: Some(crate::estimations::estimations),
            parse_mode: pob_parser::ParseMode::Full,
            skills: true,
        }
    }
}

/// Wall time per phase, milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub load_ms: f64,
    pub init_env_ms: f64,
    /// perform + the buildOutput tail.
    pub perform_ms: f64,
    /// `EstimatePlayerProgress` (+ the config rebuild when the level changed).
    pub estimate_ms: f64,
}

pub struct CalculateResult {
    /// `engine_outputJSON` of `mainOutput` (no `_meta`; that is [`CalculateResult::meta`]).
    pub stats: IndexMap<String, StatValue>,
    pub meta: EnvMeta,
    /// Every upstream branch knowingly skipped: the spec's, then the env's (initEnv, perform, buildOutput tail).
    pub unimplemented: Vec<String>,
    pub timings: Timings,
    /// The finished environment (tests, the oracle candidate); not part of the wire format.
    pub env: Env,
    /// `env.player.output` / `env.enemy.output` and the rest of what perform created.
    pub perform: PerformState,
    /// `build.characterLevel` after `EstimatePlayerProgress` (Build.lua:910-912).
    pub character_level: f64,
    /// `configTab:BuildModList()`'s lists when `EstimatePlayerProgress` changed the level (Build.lua:913), else `None`.
    pub config_rebuilt: Option<ConfigModLists>,
    /// With the skill phase on: what the [`CalcSession`] did (nested runs, cache writes, their timings); `None` on the pre-R6 path.
    pub session_log: Option<SessionLog>,
}

#[derive(Debug)]
pub struct CalcError(pub String);

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CalcError {}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// `calculate_with(xml, default options)`.
pub fn calculate(xml: &str) -> Result<CalculateResult, CalcError> {
    calculate_with(xml, CalculateOptions::default())
}

/// A build file read "as desktop" (Build.lua:2267 `io.open(path, "r")`).
pub fn calculate_file(path: impl AsRef<Path>, opts: CalculateOptions) -> Result<CalculateResult, CalcError> {
    let path = path.as_ref();
    let xml = pob_build::read_build_file(path).map_err(|e| CalcError(format!("cannot read '{}': {e}", path.display())))?;
    calculate_with(&xml, opts)
}

/// `calcs.buildOutput(build, "MAIN")` + `EstimatePlayerProgress` for one build text, taken as given. With the skill phase on (the
/// default) it runs through a fresh [`CalcSession`] (R7.2: the golden is `wipeGlobalCache` + one `buildOutput`); without it, the
/// pre-R6 path (one initEnv + one perform, the "= pob-ts" gates).
pub fn calculate_with(xml: &str, opts: CalculateOptions) -> Result<CalculateResult, CalcError> {
    let t0 = Instant::now();
    let model = pob_build::load_build_xml(xml).map_err(|e| CalcError(format!("XML: {e}")))?;
    let load_ms = ms(t0);
    if opts.skills {
        return calculate_session(Rc::new(model), opts, load_ms);
    }

    let t1 = Instant::now();
    let env_opts = InitEnvOptions {
        parse_mode: opts.parse_mode,
        ..InitEnvOptions::default()
    };
    let mut env = init_env_with(Rc::new(model), env_opts, opts.skills).map_err(|e| CalcError(format!("initEnv: {e}")))?;
    let init_env_ms = ms(t1);

    let t2 = Instant::now();
    let mut state = perform(&mut env, opts.defence, opts.estimations);
    finish_output(&mut env, &mut state);
    let perform_ms = ms(t2);

    let t3 = Instant::now();
    let (character_level, config_rebuilt) = estimate_player_progress(&mut env, &state)?;
    let estimate_ms = ms(t3);

    let stats = output_to_stats(&state.player.output);
    let meta = meta_of(&env);
    let mut unimplemented = env.spec.unimplemented.clone();
    unimplemented.extend(env.unimplemented.iter().cloned());
    Ok(CalculateResult {
        stats,
        meta,
        unimplemented,
        timings: Timings {
            load_ms,
            init_env_ms,
            perform_ms,
            estimate_ms,
        },
        env,
        perform: state,
        character_level,
        config_rebuilt,
        session_log: None,
    })
}

/// [`calculate_with`] with the skill phase: `CalcSession::build_output(MAIN)` + `EstimatePlayerProgress`.
fn calculate_session(model: Rc<pob_build::BuildModel>, opts: CalculateOptions, load_ms: f64) -> Result<CalculateResult, CalcError> {
    let t1 = Instant::now();
    let session = CalcSession::new(model, opts);
    let out = session.build_output(CalcMode::Main)?;
    let build_ms = ms(t1);
    let log = session.log().clone();
    // The cache shares the top run: dropping the session leaves the caller its only owner.
    drop(out.full_dps);
    drop(session);
    let CalcRun { mut env, state, .. } = Rc::try_unwrap(out.run)
        .map_err(|_| CalcError("calculate: the top run is still shared after the session ended".into()))?
        .into_inner();
    let init_env_ms = log.init_env_ms.first().copied().unwrap_or(0.0);

    let t3 = Instant::now();
    let (character_level, config_rebuilt) = estimate_player_progress(&mut env, &state)?;
    let estimate_ms = ms(t3);

    let stats = output_to_stats(&state.player.output);
    let meta = meta_of(&env);
    let mut unimplemented = env.spec.unimplemented.clone();
    unimplemented.extend(env.unimplemented.iter().cloned());
    Ok(CalculateResult {
        stats,
        meta,
        unimplemented,
        timings: Timings {
            load_ms,
            init_env_ms,
            perform_ms: build_ms - init_env_ms,
            estimate_ms,
        },
        env,
        perform: state,
        character_level,
        config_rebuilt,
        session_log: Some(log),
    })
}

// --- Build.lua:729-769 `acts` / `actExtra` --------------------------------------------------------------------------

/// Build.lua:729-764 `acts[act] = { level, questPoints }` -- not in `data/` (a local of Build.lua the converter does not
/// export), so ported verbatim: 11 rows, https://www.poewiki.net/wiki/Passive_skill.
const ACTS: [(f64, f64); 11] = [
    (1.0, 0.0),   // [1]
    (12.0, 2.0),  // [2]  Act 1: The Dweller of the Deep, The Marooned Mariner
    (22.0, 4.0),  // [3]  Act 1-2: The Way Forward, Through Sacred Ground
    (32.0, 6.0),  // [4]  Act 3: Victario's Secrets, Piety's Pets
    (40.0, 7.0),  // [5]  Act 4: An Indomitable Spirit
    (44.0, 9.0),  // [6]  Act 5: In Service to Science, Kitava's Torments
    (50.0, 12.0), // [7]  Act 6: The Father of War, The Puppet Mistress, The Cloven One
    (54.0, 15.0), // [8]  Act 7: The Master of a Million Faces, Queen of Despair, Kishara's Star
    (60.0, 18.0), // [9]  Act 8: Love is Dead, Reflection of Terror, The Gemling Legion
    (64.0, 20.0), // [10] Act 9: Queen of the Sands, The Ruler of Highgate
    (67.0, 23.0), // [11] Act 10: Vilenta's Vengeance, An End to Hunger (+2)
];

/// `acts[act]` (1-based, L8).
fn act_at(act: usize) -> (f64, f64) {
    ACTS[act - 1]
}

/// Build.lua:766-769 -- Act 2: Deal With The Bandits (+1 if the player kills all bandits).
fn act_extra(act: usize, extra: f64) -> f64 {
    if act > 2 { extra } else { 0.0 }
}

/// Build.lua:898-913 `buildMode:EstimatePlayerProgress()` as the load path runs it: after `calcsTab:BuildOutput()`
/// (Build.lua:695) and from `RefreshStatList` (:696, :2118). With `characterLevelAutoMode` and a changed level it sets
/// `characterLevel` and runs `configTab:BuildModList()` -- and nothing else: `SetText` is called without `notify`, and the
/// load ends with `self.buildFlag = false` (:697), so the output is NOT recalculated (the next `OnFrame` sees no
/// `buildFlag`, :1258). `mainOutput` therefore keeps the stats of the XML `level`; only the level and the config lists move.
/// Returns `(characterLevel, the rebuilt config lists when the level changed)`.
fn estimate_player_progress(env: &mut Env, state: &PerformState) -> Result<(f64, Option<ConfigModLists>), CalcError> {
    let character_level = env.build.level;
    // :900 `local PointsUsed, AscUsed, SecondaryAscUsed = self.spec:CountAllocNodes()`
    let points_used = f64::from(env.spec.count_alloc_nodes().used);
    // :901 `self.calcsTab.mainOutput and self.calcsTab.mainOutput.ExtraPoints or 0`
    let extra = state.player.output.opt_num("ExtraPoints").unwrap_or(0.0);
    // :902-908
    let mut act = 0;
    let mut level;
    loop {
        act += 1;
        let (act_level, quest_points) = act_at(act);
        level = pob_lua::number::min2(
            pob_lua::number::max2(points_used + 1.0 - quest_points - act_extra(act, extra), act_level),
            100.0,
        );
        if act == 11 || level <= act_at(act + 1).0 {
            break;
        }
    }
    // :910-913 (`controls.characterLevel:SetText` is UI only)
    if env.build.character_level_auto_mode && character_level != level {
        let lists = env
            .config
            .build_mod_list(level)
            .map_err(|e| CalcError(format!("Modules/Build.lua:913 configTab:BuildModList(): {e}")))?;
        return Ok((level, Some(lists)));
    }
    // :915-946 lab suggestion, warnings, point display -- UI only.
    Ok((character_level, None))
}
