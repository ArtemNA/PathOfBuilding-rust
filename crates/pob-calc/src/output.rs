//! What leaves the calc core (pob-ts `src/calc/output.ts`, CALC-ARCH §9): the `actor.output` table perform/defence fill,
//! and `output_to_stats` -- boot.lua:791-805 `engine_outputJSON` over the player's output.
//!
//! `actor.output` is a Lua table mixing numbers, booleans and strings (`ChaosInoculation`, `GuardSkillActive`,
//! `FortificationEffect = "0"`) and a few TABLE values ([`OutTable`]: `Req<Attr>Item`, R7.5; `SkillDPS` and the cost warning lists, R7.7);
//! table values never reach `getStats`
//! (`output_to_stats` skips them, as boot.lua's `engine_outputJSON` does) and are kept beside the scalars, under the same keys (a key
//! holds either a scalar or a table, as one Lua slot does). `output.MainHand` / `output.OffHand` live in the offence state (R7.4);
//! `output.Minion` (CalcPerform.lua:1319, the minion actor's output) is [`OutTable::Minion`] (R7.10).
//! The mod core reads the same table through `store.actor.output` (`PerStat`/`StatThreshold`/`GetStat`, ModStore.lua:
//! 308-357) as numbers only (`pob_mods::Actor::output`), so [`Output`] owns the full table and mirrors every number into the
//! mod core's actor on each write -- one Lua table, two typed views kept in step. A non-number value is invisible to the mod
//! core's view (Lua would hand GetStat a boolean there; no stat GetStat reads is ever one).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_mods::{ActorRef, Str};

/// One scalar of `actor.output`.
#[derive(Clone, Debug, PartialEq)]
pub enum OutValue {
    Num(f64),
    Bool(bool),
    Str(Str),
}

/// A table value of `actor.output`, with the structure of the Lua table it is.
#[derive(Clone, Debug)]
pub enum OutTable {
    /// CalcPerform.lua:2156 `output["Req"..attr.."Item"] = out.source` -- the `env.requirementsTable` entry itself (`Str`/`Dex`/`Int`,
    /// `source` = `"Item"` with `sourceItem`/`sourceSlot`, or `"Gem"` with `sourceGem`).
    ReqSource(crate::env::ReqEntry),
    /// Calcs.lua:395 `output.SkillDPS = fullDPS.skills` -- the `calcFullDPS` entries, in insertion order.
    SkillDps(Vec<FullDpsSkill>),
    /// Calcs.lua:420/:430 `output[<cost>.."WarningList"]` / `output[<cost>.."PercentCostWarningList"]` -- skill names, in insertion order.
    Names(Vec<String>),
    /// CalcPerform.lua:1319-1320 `output.Minion = { }` -- `env.minion.output` itself (R7.10): one table, shared with the minion's perform
    /// state.
    Minion(MinionOutput),
}

/// The minion actor's perform state as the player's `output.Minion` holds it.
#[derive(Clone)]
pub struct MinionOutput(pub Rc<std::cell::RefCell<crate::perform::ActorState>>);

impl std::fmt::Debug for MinionOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_borrow() {
            Ok(a) => write!(f, "Minion output ({} keys)", a.output.len()),
            Err(_) => write!(f, "Minion output (borrowed)"),
        }
    }
}

/// One `fullDPS.skills` entry (Calcs.lua:180/:217/:254/:298-353): `{ name, dps, count, trigger, skillPart, source }` (nil fields absent).
#[derive(Clone, Debug, PartialEq)]
pub struct FullDpsSkill {
    pub name: String,
    pub dps: f64,
    pub count: f64,
    pub trigger: Option<String>,
    pub skill_part: Option<String>,
    pub source: Option<String>,
}

/// `actor.output` (a fresh `{ }` at CalcPerform.lua:1305-1306), in assignment order.
pub struct Output {
    map: IndexMap<Str, OutValue>,
    /// The table values (a key is in at most one of `map` / `tables`).
    tables: IndexMap<Str, OutTable>,
    actor: ActorRef,
}

impl Output {
    /// `actor.output = { }`: the mod core's view is reset to the same empty table.
    pub fn new(actor: &ActorRef) -> Output {
        actor.borrow_mut().output = Some(IndexMap::new());
        Output {
            map: IndexMap::new(),
            tables: IndexMap::new(),
            actor: actor.clone(),
        }
    }

    /// `env.player.output = newEnv.player.output` (CalcMirages.lua:157/:235, R7.9): the same table becomes `actor`'s output -- here a copy of
    /// every value (scalars and table values) under `actor`, whose mod-core view is reset to its numbers. Upstream's two envs then share one
    /// table; later writes go to the copy only (what reads the mirage env's own output afterwards is listed in `crate::mirages`).
    pub fn copy_for(&self, actor: &ActorRef) -> Output {
        let nums: IndexMap<Str, f64> = self
            .map
            .iter()
            .filter_map(|(k, v)| match v {
                OutValue::Num(x) => Some((k.clone(), *x)),
                _ => None,
            })
            .collect();
        actor.borrow_mut().output = Some(nums);
        Output {
            map: self.map.clone(),
            tables: self.tables.clone(),
            actor: actor.clone(),
        }
    }

    /// The scalars of the table (a snapshot).
    pub fn scalars(&self) -> IndexMap<Str, OutValue> {
        self.map.clone()
    }

    pub fn get(&self, key: &str) -> Option<&OutValue> {
        self.map.get(key)
    }

    /// `output[key]` as a number, `None` for nil or a non-number.
    pub fn opt_num(&self, key: &str) -> Option<f64> {
        match self.map.get(key) {
            Some(OutValue::Num(v)) => Some(*v),
            _ => None,
        }
    }

    /// `output[key]` where upstream then does arithmetic on it: nil (or a non-number) is the Lua error (L18).
    pub fn num(&self, key: &str) -> f64 {
        match self.map.get(key) {
            Some(OutValue::Num(v)) => *v,
            other => panic!("Modules/CalcPerform.lua: attempt to perform arithmetic on field '{key}' ({other:?})"),
        }
    }

    pub fn set_num(&mut self, key: &str, v: f64) {
        self.drop_table(key);
        let k: Str = Rc::from(key);
        if let Some(out) = self.actor.borrow_mut().output.as_mut() {
            out.insert(k.clone(), v);
        }
        self.map.insert(k, OutValue::Num(v));
    }

    pub fn set_bool(&mut self, key: &str, v: bool) {
        self.set_other(key, OutValue::Bool(v));
    }

    pub fn set_str(&mut self, key: &str, v: &str) {
        self.set_other(key, OutValue::Str(Rc::from(v)));
    }

    /// `output[key] = nil`.
    pub fn remove(&mut self, key: &str) {
        self.drop_table(key);
        if let Some(out) = self.actor.borrow_mut().output.as_mut() {
            out.shift_remove(key);
        }
        self.map.shift_remove(key);
    }

    fn set_other(&mut self, key: &str, v: OutValue) {
        self.drop_table(key);
        if let Some(out) = self.actor.borrow_mut().output.as_mut() {
            out.shift_remove(key);
        }
        self.map.insert(Rc::from(key), v);
    }

    /// `output[key] = <table>` (the scalar under `key`, if any, is replaced -- one Lua slot).
    pub fn set_table(&mut self, key: &str, t: OutTable) {
        if let Some(out) = self.actor.borrow_mut().output.as_mut() {
            out.shift_remove(key);
        }
        self.map.shift_remove(key);
        self.tables.insert(Rc::from(key), t);
    }

    /// `output[key]` when it is a table value.
    pub fn table(&self, key: &str) -> Option<&OutTable> {
        self.tables.get(key)
    }

    /// The table values, in assignment order.
    pub fn tables(&self) -> impl Iterator<Item = (&Str, &OutTable)> {
        self.tables.iter()
    }

    fn drop_table(&mut self, key: &str) {
        if !self.tables.is_empty() {
            self.tables.shift_remove(key);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Str, &OutValue)> {
        self.map.iter()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// One `stats` value as the oracle wire format carries it (RUST-ARCH §6.3).
#[derive(Clone, Debug, PartialEq)]
pub enum StatValue {
    Num(f64),
    Str(String),
    Bool(bool),
}

/// boot.lua:791-805 `engine_outputJSON`: a number goes through as it is, except that JSON has no NaN/inf tokens, so those
/// become the strings `tostring` renders (MINEFIELD O2/O3: `"nan"`, `"inf"`, `"-inf"`); strings and booleans go through;
/// table values ([`OutTable`]) are skipped (`engine_outputJSON` writes scalars only). `-0.0` stays `-0.0` (O9).
pub fn output_to_stats(output: &Output) -> IndexMap<String, StatValue> {
    output
        .iter()
        .map(|(k, v)| {
            let s = match v {
                OutValue::Num(n) if n.is_nan() => StatValue::Str("nan".into()),
                OutValue::Num(n) if *n == f64::INFINITY => StatValue::Str("inf".into()),
                OutValue::Num(n) if *n == f64::NEG_INFINITY => StatValue::Str("-inf".into()),
                OutValue::Num(n) => StatValue::Num(*n),
                OutValue::Str(s) => StatValue::Str(s.to_string()),
                OutValue::Bool(b) => StatValue::Bool(*b),
            };
            (k.to_string(), s)
        })
        .collect()
}
