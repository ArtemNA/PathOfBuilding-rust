//! A search for better variants of a build. The engine is the only judge: every variant is a full recalculation, and a variant that breaks a
//! hard condition of [`Goal`] is dropped whatever its score. It moves passive points and chooses masteries, within the user's [`Locks`]; gems and items are not
//! touched yet.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use pob_calc::StatValue;
use pob_engine::engine::Engine;
use pob_tree::{NodeType, PassiveSpec};

/// What the user wants: hard conditions, and the weights of what to improve.
#[derive(Clone, Debug)]
pub struct Goal {
    /// Fire, cold and lightning resistance must be at least this.
    pub min_elemental_resist: f64,
    pub min_chaos_resist: Option<f64>,
    pub min_ehp: Option<f64>,
    pub dps_weight: f64,
    pub ehp_weight: f64,
    /// Gem names (auras too) a variant must not use, lower case.
    pub forbid_gems: Vec<String>,
}

/// What the search must leave alone in the tree.
#[derive(Clone, Debug, Default)]
pub struct Locks {
    /// Node ids never taken, also on the way to another node. A node the build already has is not taken away for it.
    pub forbid: HashSet<u32>,
    /// Node ids never lost, also when a node they hang on is removed.
    pub require: HashSet<u32>,
}

impl Locks {
    /// Node ids or names (several nodes can share a name); `Err` names what matches nothing, so a typo is not a lock that silently does nothing.
    pub fn resolve(spec: &PassiveSpec, forbid: &[String], require: &[String]) -> Result<Locks, String> {
        let find = |what: &str| -> Result<Vec<u32>, String> {
            let found: Vec<u32> = spec
                .nodes
                .iter()
                .filter(|n| n.dn().eq_ignore_ascii_case(what) || what.parse::<u32>().is_ok_and(|id| id == n.id))
                .map(|n| n.id)
                .collect();
            if found.is_empty() {
                return Err(format!("no tree node is called '{what}'"));
            }
            Ok(found)
        };
        let mut locks = Locks::default();
        for w in forbid {
            locks.forbid.extend(find(w)?);
        }
        for w in require {
            // only what the build has: a name shared with nodes it never took is no demand to take them
            let have: Vec<u32> = find(w)?.into_iter().filter(|id| spec.alloc_nodes.contains_key(id)).collect();
            if have.is_empty() {
                return Err(format!("the build does not have '{w}'"));
            }
            locks.require.extend(have);
        }
        Ok(locks)
    }

    /// Whether the tree `after`, made from `before`, keeps every lock.
    pub fn allow(&self, before: &PassiveSpec, after: &PassiveSpec) -> bool {
        self.require.iter().all(|id| after.alloc_nodes.contains_key(id))
            && !after
                .alloc_nodes
                .keys()
                .any(|id| self.forbid.contains(id) && !before.alloc_nodes.contains_key(id))
    }
}

/// What a variant is judged by.
#[derive(Clone, Debug, PartialEq)]
pub struct Eval {
    pub dps: f64,
    pub ehp: f64,
    pub life: f64,
    pub fire: f64,
    pub cold: f64,
    pub lightning: f64,
    pub chaos: f64,
    pub warnings: Vec<String>,
    /// The names of the enabled gems of the enabled groups, lower case.
    pub gems: Vec<String>,
}

fn stat(e: &Engine, key: &str) -> f64 {
    match e.result().stats.get(key) {
        Some(StatValue::Num(n)) => *n,
        _ => 0.0,
    }
}

/// A warning with its numbers taken out: "you are 5 Str short" and "you are 3 Str short" are the same problem.
fn warning_kind(w: &str) -> String {
    w.chars().filter(|c| !c.is_ascii_digit()).collect()
}

pub fn evaluate(e: &Engine) -> Eval {
    // The live list holds the groups items grant too, which a build gets in its XML only after the first save.
    let mut gems: Vec<String> = e
        .socket_groups()
        .iter()
        .filter(|g| g.enabled)
        .flat_map(|g| g.gem_list.iter())
        .filter(|g| g.enabled)
        .filter_map(|g| g.name_spec.as_deref())
        .map(str::to_lowercase)
        .collect();
    gems.sort_unstable();
    gems.dedup();
    Eval {
        dps: stat(e, "CombinedDPS"),
        ehp: stat(e, "TotalEHP"),
        life: stat(e, "Life"),
        fire: stat(e, "FireResist"),
        cold: stat(e, "ColdResist"),
        lightning: stat(e, "LightningResist"),
        chaos: stat(e, "ChaosResist"),
        warnings: e.stat_list().warnings.clone(),
        gems,
    }
}

/// Why a variant is not allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rejected {
    ElementalResist,
    ChaosResist,
    Ehp,
    NewWarning,
    ForbiddenGem,
    Engine,
}

impl Rejected {
    pub fn label(self) -> &'static str {
        match self {
            Rejected::ElementalResist => "elemental resist below the minimum",
            Rejected::ChaosResist => "chaos resist below the minimum",
            Rejected::Ehp => "EHP below the minimum",
            Rejected::NewWarning => "a new warning (requirements, mana, ...)",
            Rejected::ForbiddenGem => "uses a forbidden gem",
            Rejected::Engine => "the engine refused the change",
        }
    }
}

/// The first hard condition the variant breaks. A warning the starting build already has is not held against it.
pub fn violation(v: &Eval, goal: &Goal, start: &Eval) -> Option<Rejected> {
    if v.fire.min(v.cold).min(v.lightning) < goal.min_elemental_resist {
        return Some(Rejected::ElementalResist);
    }
    if goal.min_chaos_resist.is_some_and(|m| v.chaos < m) {
        return Some(Rejected::ChaosResist);
    }
    if goal.min_ehp.is_some_and(|m| v.ehp < m) {
        return Some(Rejected::Ehp);
    }
    if v.gems.iter().any(|g| goal.forbid_gems.contains(g)) {
        return Some(Rejected::ForbiddenGem);
    }
    let known: HashSet<String> = start.warnings.iter().map(|w| warning_kind(w)).collect();
    if v.warnings.iter().any(|w| !known.contains(&warning_kind(w))) {
        return Some(Rejected::NewWarning);
    }
    None
}

/// Weighted gain against the starting build: 1.0 is the starting build itself, so DPS and EHP are on one scale.
pub fn score(v: &Eval, goal: &Goal, start: &Eval) -> f64 {
    let ratio = |a: f64, b: f64| if b > 0.0 { a / b } else { 1.0 };
    let total = goal.dps_weight + goal.ehp_weight;
    if total <= 0.0 {
        return 1.0;
    }
    (goal.dps_weight * ratio(v.dps, start.dps) + goal.ehp_weight * ratio(v.ehp, start.ehp)) / total
}

/// Take back one allocated node (and what hangs on it), then spend the points on another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Move {
    pub remove: u32,
    pub add: u32,
    /// The effect chosen when `add` is a mastery.
    pub effect: Option<f64>,
}

/// Small deterministic generator: the same build and seed give the same search.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// Nodes the search may touch: the plain tree, without masteries (they need an effect choice), ascendancy, class starts and cluster subgraphs.
fn plain(spec: &PassiveSpec, i: usize) -> bool {
    let n = &spec.nodes[i];
    matches!(n.ty(), NodeType::Normal | NodeType::Notable | NodeType::Keystone)
        && !n.is_subgraph_node()
        && n.tree_node().ascendancy_name.is_none()
}

fn is_tree_mastery(spec: &PassiveSpec, i: usize) -> bool {
    let n = &spec.nodes[i];
    n.ty() == NodeType::Mastery && !n.is_subgraph_node()
}

/// At most `max` random moves that fit the points they free and keep the locks.
pub fn moves(base: &PassiveSpec, max: usize, seed: u64, locks: &Locks) -> Vec<Move> {
    /// More freed at once than this is a different kind of change, and the pairs explode.
    const MOST_POINTS_FREED: usize = 4;
    let mut rng = Rng(seed.max(1));
    let mut removable: Vec<usize> = base.alloc_nodes.values().copied().filter(|&i| plain(base, i)).collect();
    removable.retain(|&i| !locks.require.contains(&base.nodes[i].id));
    removable.sort_unstable();
    rng.shuffle(&mut removable);
    let mut all = Vec::new();
    for r in removable {
        let mut s = base.clone();
        if s.dealloc_node(r).is_err() {
            continue;
        }
        let freed = base.alloc_nodes.len().saturating_sub(s.alloc_nodes.len());
        if freed == 0 || freed > MOST_POINTS_FREED || !locks.require.iter().all(|id| s.alloc_nodes.contains_key(id)) {
            continue;
        }
        let reachable = |i: usize| {
            let n = &s.nodes[i];
            i != r
                && !n.alloc
                && n.path_dist <= freed as f64
                && n.path
                    .as_ref()
                    .is_some_and(|p| p.iter().all(|&x| !locks.forbid.contains(&s.nodes[x].id)))
        };
        let mut adds: Vec<usize> = (0..s.nodes.len()).filter(|&i| reachable(i) && plain(&s, i)).collect();
        adds.sort_unstable();
        rng.shuffle(&mut adds);
        for a in adds.into_iter().take(12) {
            all.push(Move {
                remove: base.nodes[r].id,
                add: s.nodes[a].id,
                effect: None,
            });
        }
        let mut masteries: Vec<(usize, f64)> = (0..s.nodes.len())
            .filter(|&i| reachable(i) && is_tree_mastery(&s, i))
            .flat_map(|i| {
                let taken = &s.mastery_selections;
                s.nodes[i]
                    .tree_node()
                    .mastery_effect_ids
                    .iter()
                    .filter(|e| !taken.values().any(|t| t == *e))
                    .map(move |e| (i, *e))
            })
            .collect();
        masteries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        rng.shuffle(&mut masteries);
        for (a, effect) in masteries.into_iter().take(4) {
            all.push(Move {
                remove: base.nodes[r].id,
                add: s.nodes[a].id,
                effect: Some(effect),
            });
        }
    }
    rng.shuffle(&mut all);
    all.truncate(max);
    all
}

/// The tree after a move, or why it cannot be made. It never costs more points than the tree had.
pub fn apply(base: &PassiveSpec, m: Move, locks: &Locks) -> Result<PassiveSpec, String> {
    let mut s = base.clone();
    let r = s.node_idx(m.remove).ok_or("unknown node")?;
    s.dealloc_node(r).map_err(|e| e.0)?;
    let a = s.node_idx(m.add).ok_or("unknown node")?;
    s.alloc_node(a).map_err(|e| e.0)?;
    if let Some(effect) = m.effect {
        s.mastery_selections.insert(m.add, effect);
    }
    if s.alloc_nodes.len() > base.alloc_nodes.len() {
        return Err("costs more points than were freed".into());
    }
    if !locks.allow(base, &s) {
        return Err("breaks a lock".into());
    }
    Ok(s)
}

/// The engine's full recalculation of a tree, as the application does after a click.
pub fn recalculate(e: &mut Engine, spec: PassiveSpec) -> Result<Eval, String> {
    e.write_spec_to_model(spec);
    e.refresh()?;
    if e.model().character_level_auto_mode {
        e.refresh()?;
    }
    Ok(evaluate(e))
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub seconds: f64,
    pub moves_per_round: usize,
    pub threads: usize,
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub round: usize,
    pub tried: usize,
    pub score: f64,
    pub eval: Eval,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub start: Eval,
    pub best: Eval,
    pub best_score: f64,
    pub best_xml: String,
    pub steps: Vec<Step>,
    pub tried: usize,
    pub accepted: usize,
    pub rejected: BTreeMap<Rejected, usize>,
    pub seconds: f64,
    pub load_ms: f64,
    pub calc_ms_median: f64,
}

/// One round: every move from `xml` on its own, on `threads` workers, each with its own engine (the engine is not `Send`).
fn round(xml: &str, list: &[Move], locks: &Locks, threads: usize, deadline: Instant) -> Vec<(Move, Result<Eval, String>, f64)> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(list.len()));
    std::thread::scope(|sc| {
        for _ in 0..threads.max(1) {
            sc.spawn(|| {
                let Ok(mut e) = Engine::load_xml(xml) else { return };
                let base = (*e.spec_rc()).clone();
                let mut mine = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= list.len() || Instant::now() >= deadline {
                        break;
                    }
                    let t = Instant::now();
                    let r = apply(&base, list[i], locks).and_then(|s| recalculate(&mut e, s));
                    mine.push((list[i], r, t.elapsed().as_secs_f64() * 1000.0));
                }
                if let Ok(mut o) = out.lock() {
                    o.extend(mine);
                }
            });
        }
    });
    out.into_inner().unwrap_or_default()
}

/// Greedy search: each round takes the best allowed move and goes on from there, until the time is up or no move helps.
pub fn optimize(xml: &str, goal: &Goal, locks: &Locks, opt: Options) -> Result<Report, String> {
    let begin = Instant::now();
    let deadline = begin + std::time::Duration::from_secs_f64(opt.seconds);
    let load = Instant::now();
    let mut e = Engine::load_xml(xml)?;
    let load_ms = load.elapsed().as_secs_f64() * 1000.0;
    let start = evaluate(&e);
    let mut cur_xml = xml.to_string();
    let mut cur_score = 1.0;
    let mut best = start.clone();
    let mut steps = Vec::new();
    let mut rejected = BTreeMap::new();
    let (mut tried, mut accepted) = (0, 0);
    let mut times = Vec::new();
    let mut n = 0u64;
    while Instant::now() < deadline {
        n += 1;
        let base = (*e.spec_rc()).clone();
        let list = moves(&base, opt.moves_per_round, opt.seed.wrapping_add(n), locks);
        if list.is_empty() {
            break;
        }
        let results = round(&cur_xml, &list, locks, opt.threads, deadline);
        tried += results.len();
        let mut top: Option<(f64, Move, Eval)> = None;
        for (m, r, ms) in results {
            times.push(ms);
            let v = match r {
                Ok(v) => v,
                Err(_) => {
                    *rejected.entry(Rejected::Engine).or_insert(0) += 1;
                    continue;
                }
            };
            if let Some(why) = violation(&v, goal, &start) {
                *rejected.entry(why).or_insert(0) += 1;
                continue;
            }
            accepted += 1;
            let s = score(&v, goal, &start);
            if s > cur_score && top.as_ref().is_none_or(|t| s > t.0) {
                top = Some((s, m, v));
            }
        }
        let Some((s, m, v)) = top else { break };
        let next = apply(&base, m, locks)?;
        let again = recalculate(&mut e, next)?;
        if again != v {
            return Err(format!("the same move gave different numbers on another engine: {m:?}"));
        }
        cur_xml = e.save_xml()?;
        // A fresh engine from the XML: what the user gets is what was scored.
        e = Engine::load_xml(&cur_xml)?;
        cur_score = s;
        best = v;
        steps.push(Step {
            round: n as usize,
            tried,
            score: s,
            eval: best.clone(),
        });
    }
    times.sort_by(f64::total_cmp);
    Ok(Report {
        start,
        best,
        best_score: cur_score,
        best_xml: cur_xml,
        steps,
        tried,
        accepted,
        rejected,
        seconds: begin.elapsed().as_secs_f64(),
        load_ms,
        calc_ms_median: times.get(times.len() / 2).copied().unwrap_or(0.0),
    })
}
