//! What leaves the calc core (pob-ts `src/calc/output.ts`, CALC-ARCH §9): the `actor.output` table perform/defence fill,
//! and `output_to_stats` -- boot.lua:791-805 `engine_outputJSON` over the player's output.
//!
//! `actor.output` is a Lua table mixing numbers, booleans and strings (`ChaosInoculation`, `GuardSkillActive`,
//! `FortificationEffect = "0"`); table values (`Req*Item`, `Minion`, `SkillDPS`) never reach `getStats` and are not kept.
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

/// `actor.output` (a fresh `{ }` at CalcPerform.lua:1305-1306), in assignment order.
pub struct Output {
    map: IndexMap<Str, OutValue>,
    actor: ActorRef,
}

impl Output {
    /// `actor.output = { }`: the mod core's view is reset to the same empty table.
    pub fn new(actor: &ActorRef) -> Output {
        actor.borrow_mut().output = Some(IndexMap::new());
        Output {
            map: IndexMap::new(),
            actor: actor.clone(),
        }
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
        if let Some(out) = self.actor.borrow_mut().output.as_mut() {
            out.shift_remove(key);
        }
        self.map.shift_remove(key);
    }

    fn set_other(&mut self, key: &str, v: OutValue) {
        if let Some(out) = self.actor.borrow_mut().output.as_mut() {
            out.shift_remove(key);
        }
        self.map.insert(Rc::from(key), v);
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
/// every other Lua type was never stored in [`Output`]. `-0.0` stays `-0.0` (O9).
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
