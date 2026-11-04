//! What the tree view's node tooltip asks of the build (U3a): `calcsTab:GetMiscCalculator(build)` (one per `outputRevision`) and
//! `Build:AddStatComparesToTooltip` / `CompareStatList` (Build.lua:2121-2180) over its outputs -- PassiveTreeView.lua:1683-1730.

use std::rc::Rc;

use pob_calc::env::{CalcOverride, OverrideNode};
use pob_calc::session::{CalcMode, CalcSession, MiscCalculator, RunRef};
use pob_lua::format::{Arg, format};

use crate::display::{DisplayEnv, MinionBorrow, format_num_sep, match_flags};
use crate::engine::Engine;
use crate::lua::LuaV;
use crate::stats::{DISPLAY_STATS, DisplayStat, MINION_DISPLAY_STATS, color};
use crate::view::ActorView;

/// The tooltip lines of the stat differences of one node, as `tooltip:AddLine(14, line)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeCompare {
    pub lines: Vec<String>,
    /// `isGranted` (`calcsTab.mainEnv.grantedPassives[node.id]`).
    pub is_granted: bool,
    pub count: usize,
}

/// `s_format("%+"..fmt, v)`.
fn signed(fmt: &str, v: f64) -> Result<String, String> {
    format(&format!("%+{fmt}"), &[Arg::Num(v)])
}

/// What a calculator's session reads of `calcs.buildOutput(build, "MAIN")` (Calcs.lua:386-397): the MAIN initEnv (the granted-skill merge, CalcSetup.lua:1452) and the `CALCULATOR` entries of
/// `calcFullDPS`. The tail (:400-449, every skill's MAIN run for the cost warnings) writes only `GlobalCache.cachedData.MAIN` and the MAIN output, which no calculator reads; the run shown by the
/// program has done it. Gate: `tests/misc_main_pass.rs`.
pub fn main_pass_for_calculator(sess: &CalcSession) -> Result<(), String> {
    let (env, cached) = sess.init_env_spec(CalcMode::Main, None).map_err(|e| match e {
        pob_calc::session::SessionError::Calc(e) => e.0,
        pob_calc::session::SessionError::Unimplemented(what) => format!("Calcs.lua:386 {what}"),
    })?;
    let _main = sess.perform(env, false);
    sess.calc_full_dps_spec(CalcMode::Calculator, Some(cached)).map_err(|e| e.0)?;
    Ok(())
}

impl Engine {
    pub(crate) fn misc_calculator(&self) -> Result<std::cell::Ref<'_, MiscCalculator>, String> {
        if self.misc.borrow().is_none() || self.misc_dirty.get() {
            let old = self.misc.borrow().as_ref().map(|m| (m.base.clone(), m.session().main_merge()));
            let sess = self.calc_session(self.model().clone());
            main_pass_for_calculator(&sess)?;
            let mut calc = sess.get_misc_calculator().map_err(|e| e.0)?;
            if let Some((b, merge)) = old {
                calc.base = b;
                calc.session().set_main_merge(merge);
            }
            self.misc_dirty.set(false);
            *self.misc.borrow_mut() = Some(calc);
        }
        Ok(std::cell::Ref::map(self.misc.borrow(), |m| m.as_ref().expect("just made")))
    }

    /// `Build:CompareStatList` (Build.lua:2121-2163).
    #[allow(clippy::too_many_arguments)]
    fn compare_stat_list(
        &self,
        lines: &mut Vec<String>,
        stat_list: &[DisplayStat],
        skill_flags: &indexmap::IndexMap<String, bool>,
        base: &ActorView<'_>,
        cmp: &ActorView<'_>,
        header: &str,
        node_count: Option<f64>,
    ) -> Result<usize, String> {
        let mut count = 0;
        let num = |v: LuaV<'_>| if let LuaV::Num(n) = v { n } else { 0.0 };
        for sd in stat_list {
            let Some(stat) = sd.stat else { continue };
            if !match_flags(sd.flag, sd.not_flag, skill_flags) || sd.child_stat.is_some() || stat == "SkillDPS" {
                continue;
            }
            let v1 = num(cmp.get(stat));
            let v2 = num(base.get(stat));
            let mut diff = v1 - v2;
            if stat == "FullDPS" && !cmp.has(stat) {
                diff = 0.0;
            }
            if !(diff > 0.001 || diff < -0.001) {
                continue;
            }
            let shown = match sd.cond_func {
                None => true,
                Some(f) => f(&LuaV::Num(v1), cmp)?.truthy() || f(&LuaV::Num(v2), base)?.truthy(),
            };
            if !shown {
                continue;
            }
            if count == 0 {
                lines.push(header.to_string());
            }
            let good = (sd.lower_is_better && diff < 0.0) || (!sd.lower_is_better && diff > 0.0);
            let col = if good { color::POSITIVE } else { color::NEGATIVE };
            let scale = if sd.pc || sd.mod_ { 100.0 } else { 1.0 };
            let fmt = sd.fmt.ok_or("Build.lua:2137: attempt to concatenate field 'fmt' (a nil value)")?;
            let mut val_str = signed(fmt, diff * scale)?;
            // :2138-2141 `valStr:match("^([%+%-]?%d+%.%d+)(%D*)$")`: trailing zeros of the number
            if let Some(cap) = pob_lua::match_(val_str.as_bytes(), b"^([%+%-]?%d+%.%d+)(%D*)$", None).map_err(|e| e.0)? {
                let bytes = val_str.as_bytes();
                let number = String::from_utf8_lossy(cap.get(0).and_then(|c| c.bytes(bytes)).unwrap_or(b"")).into_owned();
                let suffix = String::from_utf8_lossy(cap.get(1).and_then(|c| c.bytes(bytes)).unwrap_or(b"")).into_owned();
                let trimmed = number.trim_end_matches('0');
                let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
                val_str = format!("{trimmed}{suffix}");
            }
            let val_str = format_num_sep(&val_str, self.settings());
            let label = sd.label.unwrap_or("");
            let mut line = format!("{col}{val_str} {label}");
            let mut pc_per_pt = String::new();
            if sd.comp_percent && v1 != 0.0 && v2 != 0.0 {
                let pc = v1 / v2 * 100.0 - 100.0;
                line.push_str(&format(" (%+.1f%%)", &[Arg::Num(pc)])?);
                if let Some(n) = node_count {
                    pc_per_pt = format(" (%+.1f%%)", &[Arg::Num(pc / n)])?;
                }
            }
            if let Some(n) = node_count {
                line.push_str(&format!(" ^8[{}{pc_per_pt} per point]", signed(fmt, diff * scale / n)?));
            }
            lines.push(line);
            count += 1;
        }
        Ok(count)
    }

    /// `Build:AddStatComparesToTooltip` (Build.lua:2168-2180).
    pub(crate) fn add_stat_compares(
        &self,
        lines: &mut Vec<String>,
        base: &RunRef,
        cmp: &RunRef,
        header: &str,
        node_count: Option<f64>,
    ) -> Result<usize, String> {
        let res = self.result();
        let mb = MinionBorrow::new(res);
        let denv = DisplayEnv::new(res, &mb)?;
        let (b, c) = (base.borrow(), cmp.borrow());
        let mut count = 0;
        let mut header = header.to_string();
        if let (Some(m), Some(bm), Some(cm)) = (&denv.minion, b.state.minion.as_ref(), c.state.minion.as_ref()) {
            let (bm, cm) = (bm.borrow(), cm.borrow());
            let (bv, cv) = (ActorView::new(&bm, &b.env), ActorView::new(&cm, &c.env));
            count += self.compare_stat_list(
                lines,
                MINION_DISPLAY_STATS,
                m.skill_flags,
                &bv,
                &cv,
                &format!("{header}\n^7Minion:"),
                node_count,
            )?;
            header = if count > 0 {
                "^7Player:".to_string()
            } else {
                format!("{header}\n^7Player:")
            };
        }
        let (bv, cv) = (ActorView::new(&b.state.player, &b.env), ActorView::new(&c.state.player, &c.env));
        count += self.compare_stat_list(lines, DISPLAY_STATS, denv.player.skill_flags, &bv, &cv, &header, node_count)?;
        Ok(count)
    }

    /// `Build:AddStatComparesToTooltip(tooltip, baseOutput, compareOutput, header, nodeCount)` over the tables of `TradeQuery:ReduceOutput`: they have no `Minion`, so only the player's list.
    pub fn stat_compare_small(
        &self,
        base: &std::collections::HashMap<String, f64>,
        cmp: &std::collections::HashMap<String, f64>,
        header: &str,
        node_count: f64,
    ) -> Result<Vec<String>, String> {
        let res = self.result();
        let mb = MinionBorrow::new(res);
        let denv = DisplayEnv::new(res, &mb)?;
        let state = &res.perform.player;
        let (bv, cv) = (ActorView::small(state, &res.env, base), ActorView::small(state, &res.env, cmp));
        let mut lines = Vec::new();
        self.compare_stat_list(&mut lines, DISPLAY_STATS, denv.player.skill_flags, &bv, &cv, header, Some(node_count))?;
        Ok(lines)
    }

    /// PassiveTreeView.lua:1684-1729 on spec node `i`: the differences of allocating / deallocating it and its path. `trace_path`:
    /// `self.tracePath` (indices into `spec.nodes`).
    pub fn node_stat_compare(
        &self,
        i: usize,
        trace_path: Option<&[usize]>,
        temp: Option<Rc<pob_tree::SpecNode>>,
    ) -> Result<NodeCompare, String> {
        let calc = self.misc_calculator()?;
        let spec = self.spec_rc();
        // `temp`: PassiveMasteryControl's node with the hovered effect (`node.sd` / `modList` replaced, PassiveMasteryControl.lua:52-54)
        let node: &pob_tree::SpecNode = temp.as_deref().unwrap_or(&spec.nodes[i]);
        // the session's spec is a fresh load of the model; the live one can hold nodes it has not (a stale subgraph, TreeTab.lua:137)
        let sess_spec = calc.session().shared().map_err(|e| e.to_string())?.spec.clone();
        let key = |k: usize| -> OverrideNode {
            if k == i
                && let Some(t) = &temp
            {
                return OverrideNode::Other(t.clone());
            }
            let n = &spec.nodes[k];
            match sess_spec.node_idx(n.id) {
                Some(s) => OverrideNode::Spec(s),
                None => OverrideNode::Other(Rc::new(n.clone())),
            }
        };
        let view_tree = self.view_mode_is_tree.get();
        let path: Vec<usize> = if node.alloc {
            node.depends.clone()
        } else if let Some(t) = trace_path {
            t.to_vec()
        } else {
            node.path.clone().unwrap_or_default()
        };
        let path_length = path.len();
        let is_granted = self.result().env.granted_passives.contains_key(&node.id);
        let run = |ov: CalcOverride| -> Result<RunRef, String> { calc.calc(&Rc::new(ov), None, view_tree).map_err(|e| e.0) };
        let add = |nodes: Vec<usize>| {
            run(CalcOverride {
                add_nodes: Some(nodes.into_iter().map(key).collect()),
                ..CalcOverride::default()
            })
        };
        let remove = |nodes: Vec<usize>| {
            run(CalcOverride {
                remove_nodes: Some(nodes.into_iter().filter_map(|k| sess_spec.node_idx(spec.nodes[k].id)).collect()),
                ..CalcOverride::default()
            })
        };
        let mut path_output = None;
        let node_output;
        if node.alloc {
            node_output = remove(vec![i])?;
            if path_length > 1 {
                path_output = Some(remove(path.clone())?);
            }
        } else if is_granted {
            // `removeNodes = { [node.id] = true }` (:1706): keyed by the id, not the node: CalcSetup's `removeNodes[node]` never sees it
            node_output = run(CalcOverride::default())?;
        } else {
            if node.ty() == pob_tree::NodeType::Mastery && node.all_mastery_options {
                // :1710-1711 `pathNodes[node] = nil`, `calcFunc()`
                node_output = run(CalcOverride::default())?;
            } else {
                node_output = add(vec![i])?;
            }
            if path_length > 1 {
                let nodes: Vec<usize> = path
                    .iter()
                    .copied()
                    .filter(|p| !(node.ty() == pob_tree::NodeType::Mastery && node.all_mastery_options && *p == i))
                    .collect();
                path_output = Some(add(nodes)?);
            }
        }
        let mut out = NodeCompare {
            is_granted,
            ..NodeCompare::default()
        };
        let header = if node.alloc {
            "^7Unallocating this node will give you:"
        } else if is_granted {
            "^7This node is granted by an item. Removing it will give you:"
        } else {
            "^7Allocating this node will give you:"
        };
        let mut count = self.add_stat_compares(&mut out.lines, &calc.base, &node_output, header, None)?;
        if path_length > 1 && !is_granted && (node.intuitive_leap_likes_affecting.is_empty() || node.alloc) {
            let header = if node.alloc {
                "^7Unallocating this node and all nodes depending on it will give you:"
            } else {
                "^7Allocating this node and all nodes leading to it will give you:"
            };
            let po = path_output.as_ref().expect("path output");
            count += self.add_stat_compares(&mut out.lines, &calc.base, po, header, Some(path_length as f64))?;
        }
        out.count = count;
        Ok(out)
    }

    /// `AddStatComparesToTooltip(tooltip, base, cmp, header)` as lines.
    pub(crate) fn stat_compare_lines(&self, base: &RunRef, cmp: &RunRef, header: &str) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        self.add_stat_compares(&mut lines, base, cmp, header, None)?;
        Ok(lines)
    }

    /// `calcFunc(override)` against `calcBase` as `AddStatComparesToTooltip` lines.
    pub(crate) fn compare_override(&self, mut ov: CalcOverride, header: &str) -> Result<Vec<String>, String> {
        let calc = self.misc_calculator()?;
        // `env.flasks[override.toggleFlask]` (CalcSetup.lua:1368-1381) is a key by object, and this session parsed its own items: a live item is its twin there
        let shared = calc.session().shared().map_err(|e| e.0)?;
        for t in [&mut ov.toggle_flask, &mut ov.toggle_tincture] {
            let twin = t.as_ref().and_then(|i| {
                let id = i.id? as u64;
                self.result()
                    .env
                    .items
                    .get(&id)
                    .is_some_and(|l| Rc::ptr_eq(l, i))
                    .then(|| shared.items.get(&id).cloned())?
            });
            if twin.is_some() {
                *t = twin;
            }
        }
        let run = calc.calc(&Rc::new(ov), None, self.view_mode_is_tree.get()).map_err(|e| e.0)?;
        let mut lines = Vec::new();
        self.add_stat_compares(&mut lines, &calc.base, &run, header, None)?;
        Ok(lines)
    }

    /// `specNode.finalModList:Sum("BASE", nil, attrib)` (PassiveTreeView.lua:1450).
    pub fn node_attrib_sum(&self, id: u32, attrib: &str) -> f64 {
        let env = &self.result().env;
        if let Some(s) = env.node_final_mod_list.get(&id) {
            return s.borrow().sum(pob_mods::ModType::Base, None, &[attrib]).unwrap_or(0.0);
        }
        // CalcsTab.lua:456 `node.finalModList = node.modList` (every node, before the run)
        env.spec.node(id).map_or(0.0, |n| {
            let s = pob_mods::store::ModStore::new_list(None);
            s.borrow_mut().add_list(&n.mod_list().mods);
            s.borrow().sum(pob_mods::ModType::Base, None, &[attrib]).unwrap_or(0.0)
        })
    }

    /// `data.goldRespecPrices[build.characterLevel]`.
    pub fn gold_respec_price(&self) -> f64 {
        let level = self.model().level as usize;
        pob_data::constants::constants()
            .gold_respec_prices
            .get(level.saturating_sub(1))
            .copied()
            .unwrap_or(0.0)
    }
}
