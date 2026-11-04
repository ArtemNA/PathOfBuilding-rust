//! `actor.output` as the display functions read it: the scalars and table values of [`pob_calc::Output`], plus `output.MainHand` /
//! `output.OffHand` (kept in the offence state, [`pob_calc::ActorState::hand_output`]).

use pob_calc::{ActorState, Env};

use crate::lua::{LuaV, TableRef};

/// One actor's `output` (player or minion) and the env it belongs to (`sourceGem` of a requirement entry is a gem of the build's
/// `socketGroupList`).
pub struct ActorView<'a> {
    pub state: &'a ActorState,
    pub env: &'a Env,
    /// An output table of `TradeQuery:ReduceOutput` in place of the actor's: only its stats exist.
    small: Option<&'a std::collections::HashMap<String, f64>>,
}

impl<'a> ActorView<'a> {
    pub fn new(state: &'a ActorState, env: &'a Env) -> ActorView<'a> {
        ActorView { state, env, small: None }
    }

    /// `state`'s actor with `output` being `small` (TradeQuery.lua:808-819).
    pub fn small(state: &'a ActorState, env: &'a Env, small: &'a std::collections::HashMap<String, f64>) -> ActorView<'a> {
        ActorView {
            state,
            env,
            small: Some(small),
        }
    }

    /// `output[k]`.
    pub fn get(&self, k: &str) -> LuaV<'a> {
        if let Some(s) = self.small {
            return s.get(k).map_or(LuaV::Nil, |v| LuaV::Num(*v));
        }
        let out = &self.state.output;
        if let Some(v) = out.get(k) {
            return LuaV::from_out(v);
        }
        if let Some(t) = out.table(k) {
            return LuaV::Table(TableRef::Out(t));
        }
        let hand = match k {
            "MainHand" => 0,
            "OffHand" => 1,
            _ => return LuaV::Nil,
        };
        self.state.hand_output(hand).map_or(LuaV::Nil, |m| LuaV::Table(TableRef::Hand(m)))
    }

    /// `output[k] ~= nil`.
    pub fn has(&self, k: &str) -> bool {
        !matches!(self.get(k), LuaV::Nil)
    }

    /// `socketGroupList[g + 1].gemList[k + 1].nameSpec` (a requirement entry's `sourceGem`, CalcSetup.lua:1846-1854).
    pub fn gem_name_spec(&self, g: usize, k: usize) -> Option<String> {
        let st = self.env.skills.as_ref()?;
        st.tab.socket_group_list().get(g)?.gem_list.get(k)?.name_spec.clone()
    }
}
