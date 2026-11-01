//! R8.7 -- node power: `CalcsTab:PowerBuilder` (Classes/CalcsTab.lua:496-740), `CalculatePowerStat` (:742-746),
//! `CalculateCombinedOffDefStat` (:748-758), `data.powerStatList.GetFromOutput` (Modules/Data.lua:186-212) and
//! `TreeTab:BuildPowerReportList` (Classes/TreeTab.lua:1061-1168), over the calculator of `calcs.getMiscCalculator`
//! ([`crate::session::MiscCalculator`]). Line numbers: `grep -n` on upstream `ed354c2f8`, 2026-11-01.
//!
//! Upstream is one coroutine that yields between nodes (`engine_powerStep` in pob-engine-wasm drives it one unit at a time). Here the
//! same walk is split in two: a cheap plan ([`PowerPlan`]: the units in upstream's order and the calculator calls they need, each
//! `cache[modKey]` entry made by the first unit that asks for it, as upstream's cache) and the calls themselves. The calls only read
//! the build, so they can run in any order -- one session ([`PowerBuilder::step`], with steps and cancellation) or one session per
//! thread ([`build_power_threads`]) -- and the walk then reads their results in upstream's order: the same numbers either way.
//!
//! Orders upstream takes from a hash (D13): `pairs(self.build.spec.nodes)` and `pairs(distanceMap[dist])` (here: `spec.nodes` order),
//! `pairs(tree.clusterNodeMap)` (string keys, R13: the tree's order). They decide which node of a shared `modKey` makes the cached
//! output, the order `powerMax` sees NaNs in, and the order of equal-power entries of the report (an unstable `table.sort`).

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use pob_lua::number::max2;
use pob_tree::{NodeType, PassiveSpec};

use crate::calculate::CalcError;
use crate::env::{CalcOverride, OverrideNode};
use crate::output::{OutTable, Output};
use crate::session::{CalcMode, CalcSession, MiscCalculator, RunRef};

// --- data.powerStatList ----------------------------------------------------------------------------------------------------------

/// One `data.powerStatList` entry (Data.lua:129-180, the minion copies :225-235) -- from `data/rules.json` (`powerStatList`).
#[derive(Clone, Debug, PartialEq)]
pub struct PowerStat {
    pub stat: Option<String>,
    pub label: String,
    pub ignore_for_nodes: bool,
    /// `transform`: `"negate"` (`-value`) or `"stripThePrefix"` (the Name entry, never a node stat).
    pub transform: Option<String>,
}

/// `data.powerStatList`, in order.
pub fn power_stat_list() -> Vec<PowerStat> {
    let list = pob_data::rules::rules()
        .power_stat_list
        .as_array()
        .expect("rules.json powerStatList is an array");
    list.iter()
        .map(|e| PowerStat {
            stat: e.get("stat").and_then(|v| v.as_str()).map(str::to_owned),
            label: e.get("label").and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
            ignore_for_nodes: e.get("ignoreForNodes").and_then(|v| v.as_bool()).unwrap_or(false),
            transform: e.get("transform").and_then(|v| v.as_str()).map(str::to_owned),
        })
        .collect()
}

/// The `data.powerStatList` entry with `stat == stat`.
pub fn find_power_stat(stat: &str) -> Option<PowerStat> {
    power_stat_list().into_iter().find(|p| p.stat.as_deref() == Some(stat))
}

/// What `PowerBuilder` computes, from `self.powerStat` (:538/:547): a single stat, the Offence/Defence pair, or nothing (the Name entry).
#[derive(Clone, Debug, PartialEq)]
pub enum PowerMode {
    Stat(PowerStat),
    OffDef,
    None,
}

impl PowerMode {
    /// `self.powerStat` = `stat` (`None` = nil, the default).
    pub fn of(stat: Option<&PowerStat>) -> PowerMode {
        match stat {
            Some(p) if p.stat.is_some() && !p.ignore_for_nodes => PowerMode::Stat(p.clone()),
            Some(p) if p.ignore_for_nodes => PowerMode::None,
            _ => PowerMode::OffDef,
        }
    }
}

// --- what a power pass reads of an output -------------------------------------------------------------------------------------

/// The values `GetFromOutput` / `CalculateCombinedOffDefStat` read of one calculator output (`None` = nil); plain numbers, so a
/// result can cross threads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// `GetFromOutput(output, powerStat)` -- set in [`PowerMode::Stat`].
    pub stat: Option<f64>,
    pub life_unreserved: Option<f64>,
    pub life: Option<f64>,
    pub armour: Option<f64>,
    pub energy_shield_recovery_cap: Option<f64>,
    pub energy_shield: Option<f64>,
    pub evasion: Option<f64>,
    pub life_regen_recovery: Option<f64>,
    pub energy_shield_regen_recovery: Option<f64>,
    pub combined_dps: Option<f64>,
    /// `output.Minion and output.Minion.CombinedDPS or 0`.
    pub minion_combined_dps: f64,
}

/// `output.Minion[key]` (`None` when there is no `Minion` table or no number there).
fn minion_num(o: &Output, key: &str) -> Option<f64> {
    match o.table("Minion") {
        Some(OutTable::Minion(m)) => m.0.borrow().output.opt_num(key),
        _ => None,
    }
}

/// Data.lua:186-212 `data.powerStatList.GetFromOutput(output, statTable)` (`x and y or 0`: nil -> 0).
pub fn get_from_output(o: &Output, stat: &PowerStat) -> f64 {
    let s = stat
        .stat
        .as_deref()
        .expect("GetFromOutput: statTable.stat is nil (Data.lua:196 `statTable.stat:match`)");
    let entry = if s == "FullDPS" {
        if o.get("FullDPS").is_some() {
            o.opt_num("FullDPS").unwrap_or(0.0)
        } else {
            o.opt_num("CombinedDPS").unwrap_or(0.0) + minion_num(o, "CombinedDPS").unwrap_or(0.0)
        }
    } else if let Some(minion_stat) = s.strip_prefix("Minion").filter(|r| !r.is_empty()) {
        minion_num(o, minion_stat).unwrap_or(0.0)
    } else if s == "AverageDamage" || s == "TotalDot" || s.contains("DPS") {
        o.opt_num(s).unwrap_or(0.0) + minion_num(o, s).unwrap_or(0.0)
    } else {
        o.opt_num(s).unwrap_or(0.0)
    };
    match stat.transform.as_deref() {
        Some("negate") => -entry,
        Some(other) => panic!("Data.lua:209 transform {other} on a number stat"),
        None => entry,
    }
}

impl Sample {
    /// The values of `output` the power walk reads.
    pub fn of(o: &Output, mode: &PowerMode) -> Sample {
        Sample {
            stat: match mode {
                PowerMode::Stat(p) => Some(get_from_output(o, p)),
                _ => None,
            },
            life_unreserved: o.opt_num("LifeUnreserved"),
            life: o.opt_num("Life"),
            armour: o.opt_num("Armour"),
            energy_shield_recovery_cap: o.opt_num("EnergyShieldRecoveryCap"),
            energy_shield: o.opt_num("EnergyShield"),
            evasion: o.opt_num("Evasion"),
            life_regen_recovery: o.opt_num("LifeRegenRecovery"),
            energy_shield_regen_recovery: o.opt_num("EnergyShieldRegenRecovery"),
            combined_dps: o.opt_num("CombinedDPS"),
            minion_combined_dps: minion_num(o, "CombinedDPS").unwrap_or(0.0),
        }
    }

    fn of_run(run: &RunRef, mode: &PowerMode) -> Sample {
        Sample::of(&run.borrow().state.player.output, mode)
    }
}

/// A nil read in arithmetic: the Lua error.
fn need(v: Option<f64>, field: &str) -> f64 {
    v.unwrap_or_else(|| panic!("Classes/CalcsTab.lua:749: attempt to perform arithmetic on field '{field}' (a nil value)"))
}

/// :742-746 `CalculatePowerStat(selection, original, modified)` = `GetFromOutput(original) - GetFromOutput(modified)`.
fn power_stat(original: &Sample, modified: &Sample) -> f64 {
    original.stat.expect("stat mode") - modified.stat.expect("stat mode")
}

/// :748-758 `CalculateCombinedOffDefStat(original, modified)` -> (offence, defence).
fn combined_off_def(original: &Sample, modified: &Sample) -> (f64, f64) {
    let es = |s: &Sample| s.energy_shield_recovery_cap.or(s.energy_shield);
    let m_es = need(es(modified), "EnergyShield");
    let defence = (need(original.life_unreserved, "LifeUnreserved") - need(modified.life_unreserved, "LifeUnreserved"))
        / max2(3000.0, need(modified.life, "Life"))
        + (need(original.armour, "Armour") - need(modified.armour, "Armour")) / max2(10000.0, need(modified.armour, "Armour"))
        + (need(es(original), "EnergyShield") - m_es) / max2(3000.0, m_es)
        + (need(original.evasion, "Evasion") - need(modified.evasion, "Evasion")) / max2(10000.0, need(modified.evasion, "Evasion"))
        + (need(original.life_regen_recovery, "LifeRegenRecovery") - need(modified.life_regen_recovery, "LifeRegenRecovery")) / 500.0
        + (need(original.energy_shield_regen_recovery, "EnergyShieldRegenRecovery")
            - need(modified.energy_shield_regen_recovery, "EnergyShieldRegenRecovery"))
            / 1000.0;
    let modified_dps = need(modified.combined_dps, "CombinedDPS") + modified.minion_combined_dps;
    let dps_incr = need(original.combined_dps, "CombinedDPS") + original.minion_combined_dps - modified_dps;
    (dps_incr / modified_dps, defence)
}

// --- results ------------------------------------------------------------------------------------------------------------------

/// One `node.power.masteryEffects[effect]` entry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EffectPower {
    pub single_stat: Option<f64>,
    pub path_power: Option<f64>,
    pub offence: Option<f64>,
    pub defence: Option<f64>,
}

/// `node.power` (`None` fields = nil).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodePower {
    pub single_stat: Option<f64>,
    pub path_power: Option<f64>,
    pub offence: Option<f64>,
    pub defence: Option<f64>,
    pub distance: Option<f64>,
    /// `node.power.masteryEffects` (a table on every mastery node, :561-563): effect id -> power, in insertion order.
    pub mastery_effects: Option<IndexMap<u64, EffectPower>>,
}

/// `self.powerMax` (:500-506).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PowerMax {
    pub single_stat: f64,
    pub offence: f64,
    pub offence_per_point: f64,
    pub defence: f64,
    pub defence_per_point: f64,
}

/// What a finished `PowerBuilder` leaves: `node.power` of every spec node (by index), of every `tree.clusterNodeMap` node it computed
/// (`name -> (tree index, power)`), `powerMax`, the unit count (`total`) and the calculator calls made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerResult {
    pub nodes: Vec<NodePower>,
    pub cluster: IndexMap<String, (usize, NodePower)>,
    pub power_max: PowerMax,
    pub total: usize,
    pub calc_calls: usize,
}

// --- the plan -----------------------------------------------------------------------------------------------------------------

/// A node of `override.addNodes`, by reference into the build (so a thread can rebuild it on its own spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKey {
    /// `spec.nodes[i]`.
    Spec(usize),
    /// `buildMasteryEffectNode(spec.nodes[mastery], tree.masteryEffects[effect])` (:516-529).
    Effect { mastery: usize, effect: u64 },
    /// `tree.nodes[i]` -- a `tree.clusterNodeMap` notable.
    Tree(usize),
}

/// One calculator call: `calcFunc({ addNodes = .. })` or `calcFunc({ removeNodes = .. })`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    Add(Vec<NodeKey>),
    Remove(Vec<usize>),
}

impl Job {
    /// The `override` table on `spec`.
    fn to_override(&self, spec: &PassiveSpec) -> Rc<CalcOverride> {
        let mut ov = CalcOverride::default();
        match self {
            Job::Add(nodes) => {
                ov.add_nodes = Some(
                    nodes
                        .iter()
                        .map(|k| match *k {
                            NodeKey::Spec(i) => OverrideNode::Spec(i),
                            NodeKey::Effect { mastery, effect } => OverrideNode::Other(Rc::new(spec.mastery_effect_node(mastery, effect))),
                            NodeKey::Tree(t) => OverrideNode::Other(Rc::new(spec.tree_node_view(t))),
                        })
                        .collect(),
                );
            }
            Job::Remove(nodes) => ov.remove_nodes = Some(nodes.clone()),
        }
        Rc::new(ov)
    }
}

/// One yield point of the coroutine (a node of the distance walk, a mastery effect, a cluster notable).
#[derive(Clone, Debug)]
enum Unit {
    /// :610-662. `job`: the `cache[modKey]` / `cache[modKey.."_remove"]` call; `path_job`: the path call (stat mode only).
    Node {
        node: usize,
        distance: f64,
        job: Option<usize>,
        path_job: Option<usize>,
        add: bool,
    },
    /// :675-713, one assignable effect of an unallocated mastery (`job` None: the effect node's `modKey` is "").
    Effect {
        node: usize,
        effect: u64,
        job: Option<usize>,
        path_job: Option<usize>,
    },
    /// :718-736
    Cluster { name: String, tree_idx: usize, job: usize },
}

/// The walk of `PowerBuilder` and the calculator calls it needs, in upstream's order.
#[derive(Clone, Debug)]
pub struct PowerPlan {
    pub mode: PowerMode,
    units: Vec<Unit>,
    /// The distinct calls: `cache` entries (first asker's override) and path calls.
    pub jobs: Vec<Job>,
    /// `total` (:600-604) -- the progress denominator upstream reports.
    pub total: usize,
    /// The mastery nodes that get `node.power.masteryEffects = { }` (:561-563): every mastery.
    masteries: Vec<usize>,
    /// `node.power.distance` (:591) of every node of the distance walk, walked or not.
    distances: Vec<(usize, f64)>,
    node_count: usize,
}

/// `isValueInTable(spec.masterySelections, effect)` (:531-534): the node that has `effect` selected.
fn assigned_node(spec: &PassiveSpec, effect: u64) -> Option<u32> {
    spec.mastery_selections
        .iter()
        .find(|(_, v)| pob_tree::tree::as_id(**v).map(u64::from) == Some(effect))
        .map(|(k, _)| *k)
}

impl PowerPlan {
    /// :496-604 + the walks' structure: `granted` = `self.mainEnv.grantedPassives`, `max_depth` = `self.nodePowerMaxDepth`.
    #[allow(clippy::too_many_lines)]
    pub fn new(spec: &PassiveSpec, granted: &IndexMap<u32, bool>, mode: PowerMode, max_depth: Option<f64>) -> PowerPlan {
        let tree = spec.tree.clone();
        let is_granted = |id: u32| granted.get(&id).copied().unwrap_or(false);
        let stat_mode = matches!(mode, PowerMode::Stat(_));
        let mut jobs: Vec<Job> = Vec::new();
        let mut cache: IndexMap<String, usize> = IndexMap::new();
        let mut cached = |key: String, job: Job, jobs: &mut Vec<Job>| -> usize {
            *cache.entry(key).or_insert_with(|| {
                jobs.push(job);
                jobs.len() - 1
            })
        };
        let mut total = 0usize;
        let mut mastery_list: Vec<usize> = Vec::new();
        let mut masteries: Vec<usize> = Vec::new();
        // :559-597 `pairs(self.build.spec.nodes)` -- spec order (D13); `distanceMap[dist][nodeId]`.
        let mut distance_map: Vec<(f64, Vec<usize>)> = Vec::new();
        for (i, node) in spec.nodes.iter().enumerate() {
            if node.ty() == NodeType::Mastery {
                masteries.push(i);
            }
            if node.mod_key().is_empty() || is_granted(node.id) {
                continue;
            }
            if node.ty() == NodeType::Mastery && node.all_mastery_options {
                if !max_depth.is_some_and(|d| d < node.path_dist) {
                    mastery_list.push(i);
                    for &e in &node.tree_node().mastery_effect_ids {
                        let e = pob_tree::tree::as_id(e).map_or(u64::MAX, u64::from);
                        if assigned_node(spec, e).is_none_or(|a| a == node.id) {
                            total += 1;
                        }
                    }
                }
            } else {
                let mut dist = node.path_dist;
                for &leap in &node.intuitive_leap_likes_affecting {
                    if spec.nodes[leap].alloc {
                        dist = max2(pob_lua::number::min2(spec.nodes[leap].path_dist, dist), 1.0);
                    }
                }
                match distance_map.iter_mut().find(|(d, _)| *d == dist) {
                    Some((_, l)) => l.push(i),
                    None => distance_map.push((dist, vec![i])),
                }
                if max_depth.is_none_or(|d| dist <= d) {
                    total += 1;
                }
            }
        }
        // :598-602 `table.sort(distanceList, function(a, b) return a[1] < b[1] end)` -- distinct keys: a total order.
        distance_map.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("a distance is a number"));
        // :603-607 the eligible cluster notables
        for &ti in tree.cluster_node_map.values() {
            let tn = &tree.nodes[ti];
            if !tn.stats.mod_key.is_empty() && !is_granted(tn.id) {
                total += 1;
            }
        }

        let mut units = Vec::new();
        // :609-662
        for (distance, nodes) in &distance_map {
            if max_depth.is_some_and(|d| d < *distance) {
                break;
            }
            for &i in nodes {
                let node = &spec.nodes[i];
                let key = node.mod_key().to_owned();
                if !node.alloc {
                    let job = cached(key, Job::Add(vec![NodeKey::Spec(i)]), &mut jobs);
                    let path_job = (stat_mode && node.path.is_some() && node.ascendancy_name().is_none() && *distance > 1.0).then(|| {
                        // `for _, pathNode in pairs(node.path) do pathNodes[pathNode] = true end`
                        jobs.push(Job::Add(
                            node.path.as_ref().expect("path").iter().map(|&p| NodeKey::Spec(p)).collect(),
                        ));
                        jobs.len() - 1
                    });
                    units.push(Unit::Node {
                        node: i,
                        distance: *distance,
                        job: Some(job),
                        path_job,
                        add: true,
                    });
                } else {
                    let job = cached(format!("{key}_remove"), Job::Remove(vec![i]), &mut jobs);
                    let path_job = (stat_mode && node.ascendancy_name().is_none() && node.depends.len() > 1).then(|| {
                        jobs.push(Job::Remove(node.depends.clone()));
                        jobs.len() - 1
                    });
                    units.push(Unit::Node {
                        node: i,
                        distance: *distance,
                        job: Some(job),
                        path_job,
                        add: false,
                    });
                }
            }
        }
        // :664-716
        for &i in &mastery_list {
            let node = &spec.nodes[i];
            for &e in &node.tree_node().mastery_effect_ids {
                let Some(effect) = pob_tree::tree::as_id(e).map(u64::from) else {
                    continue;
                };
                if !assigned_node(spec, effect).is_none_or(|a| a == node.id) || !tree.mastery_effects.contains_key(&effect) {
                    continue;
                }
                let en = spec.mastery_effect_node(i, effect);
                let (job, path_job) = if en.mod_key().is_empty() {
                    (None, None)
                } else {
                    let job = cached(
                        en.mod_key().to_owned(),
                        Job::Add(vec![NodeKey::Effect { mastery: i, effect }]),
                        &mut jobs,
                    );
                    let path_job =
                        (stat_mode && node.path.is_some() && node.ascendancy_name().is_none() && node.path_dist > 1.0).then(|| {
                            let mut nodes = vec![NodeKey::Effect { mastery: i, effect }];
                            nodes.extend(
                                node.path
                                    .as_ref()
                                    .expect("path")
                                    .iter()
                                    .filter(|&&p| p != i)
                                    .map(|&p| NodeKey::Spec(p)),
                            );
                            jobs.push(Job::Add(nodes));
                            jobs.len() - 1
                        });
                    (Some(job), path_job)
                };
                units.push(Unit::Effect {
                    node: i,
                    effect,
                    job,
                    path_job,
                });
            }
        }
        // :718-736 `pairs(self.build.spec.tree.clusterNodeMap)` (R13: the tree's order)
        for (name, &ti) in &tree.cluster_node_map {
            let tn = &tree.nodes[ti];
            if tn.stats.mod_key.is_empty() || is_granted(tn.id) {
                continue;
            }
            let job = cached(tn.stats.mod_key.clone(), Job::Add(vec![NodeKey::Tree(ti)]), &mut jobs);
            units.push(Unit::Cluster {
                name: name.clone(),
                tree_idx: ti,
                job,
            });
        }
        let distances = distance_map.iter().flat_map(|(d, l)| l.iter().map(move |&i| (i, *d))).collect();
        PowerPlan {
            mode,
            units,
            jobs,
            total,
            masteries,
            distances,
            node_count: spec.nodes.len(),
        }
    }

    /// The number of units (yield points) the walk has.
    pub fn units(&self) -> usize {
        self.units.len()
    }

    /// The calls unit `u` needs.
    fn unit_jobs(&self, u: usize) -> impl Iterator<Item = usize> {
        let (a, b) = match &self.units[u] {
            Unit::Node { job, path_job, .. } | Unit::Effect { job, path_job, .. } => (*job, *path_job),
            Unit::Cluster { job, .. } => (Some(*job), None),
        };
        a.into_iter().chain(b)
    }

    /// The walk itself (:609-739) over the call results (`samples[job]`), against the base output `base`.
    #[allow(clippy::too_many_lines)]
    pub fn assemble(&self, spec: &PassiveSpec, base: &Sample, samples: &[Option<Sample>]) -> PowerResult {
        let mut out = PowerResult {
            nodes: vec![NodePower::default(); self.node_count],
            cluster: IndexMap::new(),
            power_max: PowerMax::default(),
            total: self.total,
            calc_calls: samples.iter().filter(|s| s.is_some()).count(),
        };
        let mut pm = PowerMax::default();
        for &m in &self.masteries {
            out.nodes[m].mastery_effects = Some(IndexMap::new());
        }
        for &(i, d) in &self.distances {
            out.nodes[i].distance = Some(d);
        }
        let sample = |j: usize| samples[j].as_ref().expect("every call of the walk was made");
        // `calculateAddNodePower` (:536-556)
        let add_power = |p: &mut EffectPower, distance: f64, node: usize, output: &Sample, path: Option<usize>, pm: &mut PowerMax| {
            let n = &spec.nodes[node];
            match &self.mode {
                PowerMode::Stat(_) => {
                    p.single_stat = Some(power_stat(output, base));
                    if n.path.is_some() && n.ascendancy_name().is_none() {
                        pm.single_stat = max2(pm.single_stat, p.single_stat.expect("set"));
                        p.path_power = p.single_stat;
                        if distance > 1.0 {
                            p.path_power = Some(power_stat(sample(path.expect("planned")), base));
                        }
                    }
                }
                PowerMode::OffDef => {
                    let (o, d) = combined_off_def(output, base);
                    p.offence = Some(o);
                    p.defence = Some(d);
                    p.single_stat = Some(o);
                    if n.path.is_some() && n.ascendancy_name().is_none() {
                        pm.offence = max2(pm.offence, o);
                        pm.defence = max2(pm.defence, d);
                        pm.offence_per_point = max2(pm.offence_per_point, o / distance);
                        pm.defence_per_point = max2(pm.defence_per_point, d / distance);
                    }
                }
                PowerMode::None => {}
            }
        };
        for unit in &self.units {
            match unit {
                Unit::Node {
                    node,
                    distance,
                    job,
                    path_job,
                    add,
                } => {
                    let n = &spec.nodes[*node];
                    let output = sample(job.expect("a node unit always calls"));
                    let mut p = EffectPower::default();
                    if *add {
                        add_power(&mut p, *distance, *node, output, *path_job, &mut pm);
                    } else if let PowerMode::Stat(_) = &self.mode {
                        // :628-641
                        p.single_stat = Some(power_stat(output, base));
                        if n.ascendancy_name().is_none() {
                            p.path_power = p.single_stat;
                            if n.depends.len() > 1 {
                                p.path_power = Some(power_stat(sample(path_job.expect("planned")), base));
                            }
                        }
                    }
                    let np = &mut out.nodes[*node];
                    np.single_stat = p.single_stat;
                    np.path_power = p.path_power;
                    np.offence = p.offence;
                    np.defence = p.defence;
                    // :643-652
                    if n.ty() == NodeType::Mastery
                        && let Some(&sel) = spec.mastery_selections.get(&n.id)
                    {
                        let key = pob_tree::tree::as_id(sel).map_or(u64::MAX, u64::from);
                        np.mastery_effects.get_or_insert_with(IndexMap::new).insert(key, p);
                    }
                }
                Unit::Effect {
                    node,
                    effect,
                    job,
                    path_job,
                } => {
                    let Some(job) = job else { continue };
                    let n = &spec.nodes[*node];
                    let mut ep = EffectPower::default();
                    add_power(&mut ep, n.path_dist, *node, sample(*job), *path_job, &mut pm);
                    let np = &mut out.nodes[*node];
                    match &self.mode {
                        PowerMode::Stat(_) => {
                            ep.path_power = ep.path_power.or(ep.single_stat);
                            np.single_stat = Some(max2(np.single_stat.unwrap_or(0.0), ep.single_stat.expect("set")));
                            np.path_power = Some(max2(np.path_power.unwrap_or(0.0), ep.path_power.expect("set")));
                        }
                        PowerMode::OffDef => {
                            np.offence = Some(max2(np.offence.unwrap_or(0.0), ep.offence.expect("set")));
                            np.defence = Some(max2(np.defence.unwrap_or(0.0), ep.defence.expect("set")));
                            np.single_stat = Some(max2(np.single_stat.unwrap_or(0.0), ep.single_stat.expect("set")));
                        }
                        PowerMode::None => {}
                    }
                    np.mastery_effects.get_or_insert_with(IndexMap::new).insert(*effect, ep);
                }
                Unit::Cluster { name, tree_idx, job } => {
                    let mut np = NodePower::default();
                    if let PowerMode::Stat(_) = &self.mode {
                        np.single_stat = Some(power_stat(sample(*job), base));
                    }
                    out.cluster.insert(name.clone(), (*tree_idx, np));
                }
            }
        }
        out.power_max = pm;
        out
    }
}

// --- one session: steps and cancellation ----------------------------------------------------------------------------------------

/// The power build over one session, unit by unit (`engine_powerStart` / `engine_powerStep` / `engine_powerCancel`, boot.lua:1779-1990).
pub struct PowerBuilder<'a> {
    calc: &'a MiscCalculator,
    spec: Rc<PassiveSpec>,
    pub plan: PowerPlan,
    base: Sample,
    samples: Vec<Option<Sample>>,
    next_unit: usize,
    cancelled: bool,
}

impl<'a> PowerBuilder<'a> {
    /// `SetPowerCalc(stat)` + the plan. `granted` = `mainEnv.grantedPassives` of the MAIN `buildOutput`.
    pub fn new(
        calc: &'a MiscCalculator,
        granted: &IndexMap<u32, bool>,
        stat: Option<&PowerStat>,
        max_depth: Option<f64>,
    ) -> Result<PowerBuilder<'a>, CalcError> {
        let spec = calc.session().shared().map_err(|e| CalcError(format!("power: {e}")))?.spec.clone();
        let mode = PowerMode::of(stat);
        let plan = PowerPlan::new(&spec, granted, mode.clone(), max_depth);
        let base = Sample::of_run(&calc.base, &mode);
        let samples = vec![None; plan.jobs.len()];
        Ok(PowerBuilder {
            calc,
            spec,
            plan,
            base,
            samples,
            next_unit: 0,
            cancelled: false,
        })
    }

    /// Units done so far, and the unit count.
    pub fn progress(&self) -> (usize, usize) {
        (self.next_unit, self.plan.units())
    }

    pub fn is_done(&self) -> bool {
        self.cancelled || self.next_unit >= self.plan.units()
    }

    /// `engine_powerCancel`: the build is abandoned (no partial result).
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Runs the calls of the next `n` units (`engine_powerStep(n)`); returns whether the walk is done.
    pub fn step(&mut self, n: usize) -> Result<bool, CalcError> {
        let end = (self.next_unit + n).min(self.plan.units());
        while !self.cancelled && self.next_unit < end {
            let jobs: Vec<usize> = self.plan.unit_jobs(self.next_unit).collect();
            for j in jobs {
                if self.samples[j].is_none() {
                    let ov = self.plan.jobs[j].to_override(&self.spec);
                    // `useFullDPS = self.powerStat and self.powerStat.stat == "FullDPS"` (:498): nil / false / true.
                    let run = self.calc.calc(&ov, use_full_dps(&self.plan.mode), false)?;
                    self.samples[j] = Some(Sample::of_run(&run, &self.plan.mode));
                }
            }
            self.next_unit += 1;
        }
        Ok(self.is_done())
    }

    /// The finished walk (`None` if cancelled or not done).
    pub fn result(&self) -> Option<PowerResult> {
        (!self.cancelled && self.next_unit >= self.plan.units()).then(|| self.plan.assemble(&self.spec, &self.base, &self.samples))
    }
}

/// `useFullDPS` of :498 for `calcFunc(override, useFullDPS)`.
fn use_full_dps(mode: &PowerMode) -> Option<bool> {
    match mode {
        PowerMode::Stat(p) => Some(p.stat.as_deref() == Some("FullDPS")),
        PowerMode::OffDef => None,
        // `self.powerStat` is the Name entry: `stat == nil` -> false.
        PowerMode::None => Some(false),
    }
}

// --- one session per thread -----------------------------------------------------------------------------------------------------

/// A session in the state `CalcsTab:BuildOutput` leaves: `buildOutput(MAIN)`, then `getMiscCalculator`.
fn ready_session(
    build: &pob_build::BuildModel,
    opts: crate::calculate::CalculateOptions,
) -> Result<(MiscCalculator, IndexMap<u32, bool>), String> {
    let sess = CalcSession::new(Rc::new(build.clone()), opts);
    let main = sess.build_output(CalcMode::Main).map_err(|e| e.0)?;
    let granted = main.run.borrow().env.granted_passives.clone();
    let calc = sess.get_misc_calculator().map_err(|e| e.0)?;
    Ok((calc, granted))
}

/// The whole power build on `threads` threads (`threads <= 1`: this thread only). Each thread makes its own session of `build` in the
/// state `CalcsTab:BuildOutput` leaves (`buildOutput(MAIN)` + `getMiscCalculator`) and takes calls off one shared counter; the walk
/// then reads the results in upstream's order -- the result is the one-thread result. `cancel` stops it (`Ok(None)`).
pub fn build_power_threads(
    build: &pob_build::BuildModel,
    opts: crate::calculate::CalculateOptions,
    stat: Option<&PowerStat>,
    max_depth: Option<f64>,
    threads: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Option<PowerResult>, String> {
    let (calc, granted) = ready_session(build, opts)?;
    let spec = calc.session().shared().map_err(|e| e.to_string())?.spec.clone();
    let mode = PowerMode::of(stat);
    let plan = PowerPlan::new(&spec, &granted, mode.clone(), max_depth);
    let base = Sample::of_run(&calc.base, &mode);
    let n_jobs = plan.jobs.len();
    let jobs = Arc::new(plan.jobs.clone());
    let results: Arc<Mutex<Vec<Option<Sample>>>> = Arc::new(Mutex::new(vec![None; n_jobs]));
    let next = Arc::new(AtomicUsize::new(0));
    let failed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let stop = AtomicBool::new(false);
    let ufd = use_full_dps(&mode);
    let work = |calc: &MiscCalculator, spec: &PassiveSpec| {
        loop {
            if stop.load(Ordering::Relaxed) || cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return;
            }
            let j = next.fetch_add(1, Ordering::Relaxed);
            if j >= n_jobs {
                return;
            }
            let ov = jobs[j].to_override(spec);
            match calc.calc(&ov, ufd, false) {
                Ok(run) => {
                    let s = Sample::of_run(&run, &mode);
                    results.lock().expect("results")[j] = Some(s);
                }
                Err(e) => {
                    *failed.lock().expect("failed") = Some(e.0);
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads.max(1) {
            let work = &work;
            let failed = failed.clone();
            let stop = &stop;
            scope.spawn(move || {
                // `BuildModel` is plain data; every `Rc` of a session stays on its thread.
                match ready_session(build, opts) {
                    Ok((calc, _)) => match calc.session().shared() {
                        Ok(sh) => {
                            let spec = sh.spec.clone();
                            work(&calc, &spec);
                        }
                        Err(e) => {
                            *failed.lock().expect("failed") = Some(e.to_string());
                            stop.store(true, Ordering::Relaxed);
                        }
                    },
                    Err(e) => {
                        *failed.lock().expect("failed") = Some(e);
                        stop.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
        work(&calc, &spec);
    });
    if let Some(e) = failed.lock().expect("failed").take() {
        return Err(e);
    }
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Ok(None);
    }
    let samples = results.lock().expect("results").clone();
    Ok(Some(plan.assemble(&spec, &base, &samples)))
}

// --- TreeTab:BuildPowerReportList -----------------------------------------------------------------------------------------------

/// What `BuildPowerReportList` reads of the `build.displayStats` entry for the stat (TreeTab.lua:1072-1086); `None` = its
/// `{ fmt = ".1f" }` fallback.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayStatFmt {
    pub fmt: String,
    pub pc: bool,
    pub mod_: bool,
    pub lower_is_better: bool,
}

/// `main.showThousandsSeparators` / `thousandsSeparator` / `decimalSeparator` (Main.lua settings).
#[derive(Clone, Debug, PartialEq)]
pub struct NumberFormat {
    pub show_thousands_separators: bool,
    pub thousands_separator: String,
    pub decimal_separator: String,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            show_thousands_separators: true,
            thousands_separator: ",".into(),
            decimal_separator: ".".into(),
        }
    }
}

/// `pathDist` of a report entry: a number, or the `"Cluster"` sentinel.
#[derive(Clone, Debug, PartialEq)]
pub enum ReportDist {
    Num(f64),
    Str(&'static str),
}

/// One `report` entry (TreeTab.lua:1105-1119).
#[derive(Clone, Debug, PartialEq)]
pub struct ReportEntry {
    pub name: String,
    pub power: f64,
    pub power_str: String,
    pub path_power: f64,
    pub path_power_str: String,
    pub allocated: bool,
    pub id: u32,
    pub ty: NodeType,
    pub path_dist: ReportDist,
}

fn gsub_str(s: &[u8], pat: &str, repl: &[u8]) -> Vec<u8> {
    pob_lua::gsub(s, pat.as_bytes(), pob_lua::Repl::Str(repl), None).expect("pattern").0
}

fn lua_match<'s>(s: &'s [u8], pat: &str) -> Option<&'s [u8]> {
    let caps = pob_lua::match_(s, pat.as_bytes(), None).expect("pattern")?;
    caps.get(0)?.bytes(s)
}

/// `utf8.reverse`: the integer part is ASCII digits and separator bytes; reversed by UTF-8 character.
fn utf8_reverse(s: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(s) {
        Ok(t) => t.chars().rev().collect::<String>().into_bytes(),
        Err(_) => s.iter().rev().copied().collect(),
    }
}

/// Common.lua:803-829 `formatNumSep(str)` -- the single implementation (the sidebar, the Calcs tab, node power and the offence strings;
/// R8.8 merged the copy `pob-engine` had). Separators may be any UTF-8 text (`utf8.reverse` / `utf8.sub`).
pub fn format_num_sep(s: &str, nf: &NumberFormat) -> String {
    let mut f = |subject: &[u8], caps: &pob_lua::Captures| -> Option<Vec<u8>> {
        let m = caps.get(0).and_then(|c| c.bytes(subject)).unwrap_or(subject);
        let colour: Vec<u8> = lua_match(m, "(^x%x%x%x%x%x%x)")
            .or_else(|| lua_match(m, "(%^%d)"))
            .map_or_else(Vec::new, <[u8]>::to_vec);
        let stripped = gsub_str(&gsub_str(m, "(^x%x%x%x%x%x%x)", b""), "(%^%d)", b"");
        if stripped.is_empty() || (colour.is_empty() && lua_match(m, "%^").is_some()) {
            return Some(m.to_vec());
        }
        let (_, _, c) = pob_lua::find(&stripped, b"(-?)(%d+)(%.?%d*)", None, false).expect("pattern")?;
        let part = |i: usize| c.get(i).and_then(|x| x.bytes(&stripped)).unwrap_or(b"").to_vec();
        let (minus, mut integer, fraction) = (part(0), part(1), part(2));
        if nf.show_thousands_separators {
            let rev_sep = utf8_reverse(nf.thousands_separator.as_bytes());
            let mut repl = b"%1".to_vec();
            repl.extend_from_slice(&rev_sep);
            integer = utf8_reverse(&gsub_str(&utf8_reverse(&integer), "(%d%d%d)", &repl));
            if !nf.thousands_separator.is_empty() && integer.starts_with(&rev_sep) {
                // `utf8.sub(integer, 2)`: drop the first character
                let first = std::str::from_utf8(&integer)
                    .ok()
                    .and_then(|t| t.chars().next())
                    .map_or(1, char::len_utf8);
                integer.drain(..first);
            }
        } else {
            integer = utf8_reverse(&gsub_str(&utf8_reverse(&integer), "(%d%d%d)", b"%1"));
        }
        let mut out = colour;
        out.extend_from_slice(&minus);
        out.extend_from_slice(&integer);
        out.extend_from_slice(&gsub_str(&fraction, "%.", nf.decimal_separator.as_bytes()));
        Some(out)
    };
    let (out, _) = pob_lua::gsub(
        s.as_bytes(),
        b"(%^?x?%x?%x?%x?%x?%x?%x?-?%d+%.?%d+)",
        pob_lua::Repl::Fn(&mut f),
        None,
    )
    .expect("formatNumSep pattern");
    String::from_utf8(out).expect("formatNumSep keeps UTF-8")
}

/// TreeTab.lua:1061-1168 `BuildPowerReportList(currentStat)`: `granted` = `mainEnv.grantedPassives`. The final `table.sort` is unstable
/// upstream; here a stable sort over the spec order, then the cluster notables in the tree's order (equal powers: D13).
pub fn build_power_report_list(
    spec: &PassiveSpec,
    granted: &IndexMap<u32, bool>,
    result: &PowerResult,
    current: Option<&PowerStat>,
    display: Option<&DisplayStatFmt>,
    nf: &NumberFormat,
) -> Vec<ReportEntry> {
    let mut report = Vec::new();
    if current.is_none_or(|c| c.stat.is_none()) {
        return report;
    }
    let fallback = DisplayStatFmt {
        fmt: ".1f".into(),
        pc: false,
        mod_: false,
        lower_is_better: false,
    };
    let ds = display.unwrap_or(&fallback);
    let mult = if ds.pc || ds.mod_ { 100.0 } else { 1.0 };
    let codes = &pob_data::globals::globals().color_codes.0;
    let format_power = |power: f64| -> String {
        let raw = pob_lua::format(&format!("%{}", ds.fmt), &[pob_lua::Arg::Num(power)]).expect("string.format");
        let s = format_num_sep(&raw, nf);
        let lower = ds.lower_is_better;
        if (power > 0.0 && !lower) || (power < 0.0 && lower) {
            format!("{}{s}", codes["POSITIVE"])
        } else if (power < 0.0 && !lower) || (power > 0.0 && lower) {
            format!("{}{s}", codes["NEGATIVE"])
        } else {
            s
        }
    };
    let tree = &spec.tree;
    for (i, node) in spec.nodes.iter().enumerate() {
        let is_alloc = node.alloc || granted.get(&node.id).copied().unwrap_or(false);
        let np = &result.nodes[i];
        // `getNodePathDist(node, isAlloc)` (:1100-1105)
        let path_dist = if is_alloc {
            if node.depends.is_empty() { 1.0 } else { node.depends.len() as f64 }
        } else {
            match np.distance {
                Some(d) => d,
                None => match node.path.as_ref().map_or(0, Vec::len) {
                    0 => 1.0,
                    n => n as f64,
                },
            }
        };
        let ty = node.ty();
        let mut add = |name: String, power: f64, path_power: f64| {
            report.push(ReportEntry {
                name,
                power,
                power_str: format_power(power),
                path_power,
                path_power_str: format_power(path_power),
                allocated: is_alloc,
                id: node.id,
                ty,
                path_dist: ReportDist::Num(path_dist),
            });
        };
        if matches!(ty, NodeType::Normal | NodeType::Keystone | NodeType::Notable) && node.ascendancy_name().is_none() {
            add(
                node.dn().to_owned(),
                np.single_stat.unwrap_or(0.0) * mult,
                np.path_power.unwrap_or(0.0) / path_dist * mult,
            );
        } else if ty == NodeType::Mastery
            && let Some(effects) = &np.mastery_effects
            && node.ascendancy_name().is_none()
        {
            for &e in &node.tree_node().mastery_effect_ids {
                let Some(key) = pob_tree::tree::as_id(e).map(u64::from) else {
                    continue;
                };
                let (Some(effect), Some(ep)) = (tree.mastery_effects.get(&key), effects.get(&key)) else {
                    continue;
                };
                // `isAlloc and not node.allMasteryOptions and node.sd or effect.stats or effect.sd` (`effect.stats` is nil)
                let parts: &[String] = if is_alloc && !node.all_mastery_options {
                    node.sd()
                } else {
                    &effect.sd
                };
                let name = format!("{}: {}", node.dn(), parts.join(" / "));
                add(
                    name,
                    ep.single_stat.unwrap_or(0.0) * mult,
                    (ep.path_power.or(ep.single_stat).unwrap_or(0.0) / path_dist) * mult,
                );
            }
        }
    }
    // :1147-1154 the cluster notables (`node.alloc` is nil on a tree node)
    for &ti in tree.cluster_node_map.values() {
        let tn = &tree.nodes[ti];
        let power = result
            .cluster
            .values()
            .find(|(t, _)| *t == ti)
            .and_then(|(_, p)| p.single_stat)
            .unwrap_or(0.0)
            * mult;
        report.push(ReportEntry {
            name: tn.dn.clone(),
            power,
            power_str: format_power(power),
            path_power: 0.0,
            path_power_str: "--".into(),
            allocated: false,
            id: tn.id,
            ty: tn.ty,
            path_dist: ReportDist::Str("Cluster"),
        });
    }
    // :1157-1165
    if ds.lower_is_better {
        report.sort_by(|a, b| a.power.partial_cmp(&b.power).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        report.sort_by(|a, b| b.power.partial_cmp(&a.power).unwrap_or(std::cmp::Ordering::Equal));
    }
    report
}
