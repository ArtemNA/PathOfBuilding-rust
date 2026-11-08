//! A search for better variants of a build. The engine is the only judge: every variant is a full recalculation, and a variant that breaks a
//! hard condition of [`Goal`] is dropped whatever its score. It moves passive points, chooses masteries, swaps support gems of the main skill and tries unique items, within the user's
//! [`Locks`]. A config option that only made sense with what a change removed is taken off ([`ConfigGuard`]): upstream keeps applying a hidden option otherwise.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use pob_build::InputValue;
use pob_calc::StatValue;
use pob_data::config_options::{ConfigVar, config_options};
use pob_engine::config_tab::ConfigGate;
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
    /// Whether support gems of the main group may be swapped. The active gem is never touched.
    pub swap_supports: bool,
    /// Gear slots that may get another item; empty = none.
    pub item_slots: Vec<String>,
    /// Unique names never tried, lower case.
    pub forbid_items: Vec<String>,
    /// Off: the build's tattoos stay and no new one is tried. On: every round holds variants with new ones.
    pub tattoos: bool,
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
        if !self.tattoos && !keeps_tattoos(before, after) {
            return false;
        }
        self.require.iter().all(|id| after.alloc_nodes.contains_key(id))
            && !after
                .alloc_nodes
                .keys()
                .any(|id| self.forbid.contains(id) && !before.alloc_nodes.contains_key(id))
    }
}

/// Every tattooed node the tree had allocated is still allocated.
fn keeps_tattoos(before: &PassiveSpec, after: &PassiveSpec) -> bool {
    before
        .hash_overrides
        .keys()
        .all(|id| !before.alloc_nodes.contains_key(id) || after.alloc_nodes.contains_key(id))
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
    /// Config options taken off because the change removed their reason.
    pub dropped_config: Vec<String>,
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
        dropped_config: Vec::new(),
    }
}

fn truthy(v: Option<&InputValue>) -> bool {
    !matches!(v, None | Some(InputValue::Bool(false)))
}

fn opts(j: &serde_json::Value) -> Vec<String> {
    match j {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

/// `control.shown` of one option (ConfigTab.lua:353-364, the `if*` keys of :382-662), without the search box and the "show all" toggle. An unknown key counts as shown.
fn var_shown(e: &Engine, v: &ConfigVar) -> bool {
    let transfigured = v.gates.get("includeTransfigured").is_some_and(|x| x.as_bool() == Some(true));
    let input = &e.config_state().input;
    let var_set = v.var.as_ref().is_some_and(|n| truthy(input.get(n)));
    let imply: Vec<String> = ["implyCond", "implyCondList"]
        .iter()
        .filter_map(|k| v.gates.get(*k))
        .flat_map(opts)
        .collect();
    let implied = var_set && imply.iter().any(|c| e.config_gate(ConfigGate::Cond, c));
    v.gates.iter().all(|(k, val)| {
        let (gate, uses_imply) = match k.as_str() {
            "ifOption" => return opts(val).iter().any(|o| truthy(input.get(o))),
            "ifCond" => (ConfigGate::Cond, true),
            "ifMinionCond" => (ConfigGate::MinionCond, true),
            "ifEnemyCond" => (ConfigGate::EnemyCond, true),
            "ifCondTrue" => (ConfigGate::CondTrue, false),
            "ifMult" => (ConfigGate::Mult, true),
            "ifEnemyMult" => (ConfigGate::EnemyMult, true),
            "ifStat" => (ConfigGate::Stat, true),
            "ifTagType" => (ConfigGate::TagType, true),
            "ifFlag" => (ConfigGate::Flag, false),
            "ifMod" => (ConfigGate::Mod, true),
            "ifSkill" if transfigured => (ConfigGate::SkillTransfigured, false),
            "ifSkill" => (ConfigGate::Skill, false),
            "ifSkillFlag" => (ConfigGate::SkillFlag, false),
            "ifSkillData" => (ConfigGate::SkillData, false),
            _ => return true,
        };
        (uses_imply && implied) || opts(val).iter().any(|o| e.config_gate(gate, o))
    })
}

/// The config options the user set and the build showed at the start. Upstream's `BuildModList` applies an option whether it is shown or not, so an option whose reason
/// has gone (the item with that condition was replaced) would still add its effect: such an option goes with its reason. An option already hidden at the start was the user's own choice, and stays.
#[derive(Clone, Debug, Default)]
pub struct ConfigGuard {
    watched: Vec<String>,
}

impl ConfigGuard {
    pub fn new(e: &Engine) -> ConfigGuard {
        let input = &e.config_state().input;
        let watched = config_options()
            .vars
            .iter()
            .filter(|v| !v.gates.is_empty())
            .filter(|v| {
                v.var
                    .as_ref()
                    .is_some_and(|n| input.get(n).is_some_and(|cur| *cur != e.config_default_state(n, cur)))
            })
            .filter(|v| var_shown(e, v))
            .filter_map(|v| v.var.clone())
            .collect();
        ConfigGuard { watched }
    }

    pub fn watched(&self) -> &[String] {
        &self.watched
    }

    /// The watched options that are hidden now.
    pub fn orphans(&self, e: &Engine) -> Vec<String> {
        self.watched
            .iter()
            .filter(|name| {
                config_options()
                    .vars
                    .iter()
                    .find(|v| v.var.as_deref() == Some(name.as_str()))
                    .is_some_and(|v| !var_shown(e, v))
            })
            .cloned()
            .collect()
    }

    /// Takes the orphans off and recalculates. Hiding one can hide another (`ifOption`), so it goes on until none is left.
    pub fn settle(&self, e: &mut Engine) -> Result<Vec<String>, String> {
        let mut dropped = Vec::new();
        loop {
            let now: Vec<String> = self.orphans(e).into_iter().filter(|n| !dropped.contains(n)).collect();
            if now.is_empty() {
                return Ok(dropped);
            }
            for n in now {
                e.set_config(&n, None)?;
                dropped.push(n);
            }
            settle(e)?;
        }
    }
}

fn settle(e: &mut Engine) -> Result<(), String> {
    e.refresh()?;
    if e.model().character_level_auto_mode {
        e.refresh()?;
    }
    Ok(())
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
        if !locks.tattoos && !keeps_tattoos(base, &s) {
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
    settle(e)?;
    Ok(evaluate(e))
}

/// One change a variant is made of.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Tree(Move),
    /// Another support gem in slot `gem` of socket group `group` (both 1-based).
    Support {
        group: usize,
        gem: usize,
        gem_id: String,
    },
    /// The tattoo called `dn` on the allocated node `node`.
    Tattoo {
        node: u32,
        dn: String,
    },
    /// `raw` (an item's text) in the gear slot.
    Item {
        slot: String,
        name: String,
        raw: String,
    },
}

impl Change {
    pub fn describe(&self, base: &PassiveSpec) -> String {
        let name = |id: u32| base.node(id).map_or_else(|| id.to_string(), |n| n.dn().to_string());
        match self {
            Change::Tree(m) => format!("tree: {} -> {}", name(m.remove), name(m.add)),
            Change::Support { group, gem, gem_id } => {
                let shown = pob_data::gems::gems()
                    .gems
                    .0
                    .get(gem_id)
                    .map_or(gem_id.as_str(), |g| g.name.as_str());
                format!("gem: group {group} slot {gem} -> {shown}")
            }
            Change::Tattoo { node, dn } => format!("tattoo: {} -> {dn}", name(*node)),
            Change::Item { slot, name, .. } => format!("item: {slot} -> {name}"),
        }
    }
}

/// Makes the change on an engine holding the build, recalculates and takes off what lost its reason.
pub fn try_change(e: &mut Engine, base: &PassiveSpec, c: &Change, locks: &Locks, guard: &ConfigGuard) -> Result<Eval, String> {
    match c {
        Change::Tree(m) => e.write_spec_to_model(apply(base, *m, locks)?),
        Change::Support { group, gem, gem_id } => {
            e.set_gem_id(*group, *gem, Some(gem_id))?;
            e.set_gem_quality(*group, *gem, Some(20.0))?;
        }
        Change::Tattoo { node, dn } => {
            let mut s = base.clone();
            let tattoo = s.tree.tattoo().map_err(|x| x.0)?.nodes.get(dn).cloned().ok_or("unknown tattoo")?;
            s.hash_overrides.insert(*node, tattoo);
            e.write_spec_to_model(s);
        }
        Change::Item { slot, raw, .. } => {
            e.replace_item_text(raw, slot)?;
        }
    }
    settle(e)?;
    let dropped = guard.settle(e)?;
    let mut v = evaluate(e);
    v.dropped_config = dropped;
    Ok(v)
}

/// A support swap never picks these by itself: they are the expensive or special ones.
fn plain_support(name: &str) -> bool {
    let n = name.to_lowercase();
    !(n.starts_with("awakened ") || n.starts_with("vaal ") || ["empower", "enlighten", "enhance"].iter().any(|x| n.contains(x)))
}

/// The game's limit of tattoos on one character.
const MOST_TATTOOS: usize = 50;

/// The number in a tattoo's "Limited to N" line.
fn limit_of(sd: &[String]) -> Option<usize> {
    sd.iter().find_map(|l| {
        l.strip_prefix("Limited to ")
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
    })
}

/// Tattoos the build could still take, on nodes it has: the test of TreeTab.lua:856-859, without the legacy ones.
fn tattoo_changes(base: &PassiveSpec, max: usize, rng: &mut Rng) -> Result<Vec<Change>, String> {
    if base.hash_overrides.len() >= MOST_TATTOOS {
        return Ok(Vec::new());
    }
    let data = pob_data::tattoo::tattoo_passives().map_err(|e| e.0)?;
    let matches = |s: &str, pat: &str| matches!(pob_lua::match_(s.as_bytes(), pat.as_bytes(), None), Ok(Some(_)));
    let mut out = Vec::new();
    for &i in base.alloc_nodes.values() {
        let n = &base.nodes[i];
        if base.hash_overrides.contains_key(&n.id) || n.is_subgraph_node() || n.tree_node().ascendancy_name.is_some() {
            continue;
        }
        let tree = n.tree_node();
        let value = tree.sd.first().map_or("", String::as_str);
        let linked = n.linked.len() as f64;
        for (_, t) in data.nodes.iter() {
            let kind = t.target_type.strip_prefix("Small ").unwrap_or(&t.target_type);
            let fits = matches(&tree.dn, kind)
                || (!t.target_value.is_empty() && matches(value, &t.target_value))
                || (t.target_type == "Small Attribute" && ["Intelligence", "Strength", "Dexterity"].contains(&tree.dn.as_str()))
                || (t.target_type == "Keystone" && n.ty() == NodeType::Keystone);
            if !fits || t.minimum_connected > linked || t.legacy == Some(true) {
                continue;
            }
            let used = base.hash_overrides.values().filter(|o| o.dn == t.dn).count();
            if limit_of(&t.sd).is_some_and(|l| used >= l) {
                continue;
            }
            out.push(Change::Tattoo {
                node: n.id,
                dn: t.dn.clone(),
            });
        }
    }
    out.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    rng.shuffle(&mut out);
    out.truncate(max);
    Ok(out)
}

fn support_changes(e: &Engine, goal: &Goal, max: usize, rng: &mut Rng) -> Vec<Change> {
    let group = e.model().main_socket_group as usize;
    let db = &pob_data::gems::gems().gems.0;
    let allowed: Vec<&str> = db
        .iter()
        .filter(|(_, g)| plain_support(&g.name) && !goal.forbid_gems.contains(&g.name.to_lowercase()))
        .map(|(id, _)| id.as_str())
        .collect();
    let Ok(can) = e.gem_can_support(group, &allowed, false) else {
        return Vec::new();
    };
    let states = e.gem_states(group);
    let have: HashSet<String> = (1..=states.len()).filter_map(|g| e.gem_id_at(group, g)).collect();
    let mut ids: Vec<String> = can.into_iter().filter(|id| !have.contains(id)).collect();
    ids.sort_unstable();
    let mut out = Vec::new();
    for (i, s) in states.iter().enumerate() {
        if !s.3 {
            continue;
        }
        for id in &ids {
            out.push(Change::Support {
                group,
                gem: i + 1,
                gem_id: id.clone(),
            });
        }
    }
    rng.shuffle(&mut out);
    out.truncate(max);
    out
}

/// (name, slots, text) of every unique a gear slot can take, in the database's order.
pub fn uniques() -> Result<Vec<(String, Vec<String>, String)>, String> {
    let j: serde_json::Value = pob_data::read_json("uniques.json").map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for raws in j["byType"].as_object().into_iter().flatten().map(|(_, v)| v) {
        for raw in raws.as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            let item = pob_items::Item::parse(&pob_build::sanitise_text(raw), &pob_items::ItemOptions::default());
            let Some(base) = item.base else { continue };
            let slots: Vec<String> = match base.ty.as_str() {
                "Ring" => vec!["Ring 1".into(), "Ring 2".into()],
                "Helmet" | "Gloves" | "Boots" | "Amulet" | "Belt" | "Body Armour" => vec![base.ty.clone()],
                _ => continue,
            };
            out.push((item.name.clone(), slots, raw.to_string()));
        }
    }
    Ok(out)
}

fn item_changes(db: &[(String, Vec<String>, String)], locks: &Locks, max: usize, rng: &mut Rng) -> Vec<Change> {
    let mut out = Vec::new();
    for (name, slots, raw) in db {
        if locks.forbid_items.contains(&name.to_lowercase()) {
            continue;
        }
        for s in slots.iter().filter(|s| locks.item_slots.contains(s)) {
            out.push(Change::Item {
                slot: s.clone(),
                name: name.clone(),
                raw: raw.clone(),
            });
        }
    }
    rng.shuffle(&mut out);
    out.truncate(max);
    out
}

/// The changes of one round: a quarter at most for gems and for items, the rest for the tree.
pub fn candidates(e: &Engine, locks: &Locks, db: &[(String, Vec<String>, String)], max: usize, seed: u64) -> Result<Vec<Change>, String> {
    candidates_for(e, &NO_GOAL, locks, db, max, seed)
}

const NO_GOAL: Goal = Goal {
    min_elemental_resist: f64::NEG_INFINITY,
    min_chaos_resist: None,
    min_ehp: None,
    dps_weight: 1.0,
    ehp_weight: 1.0,
    forbid_gems: Vec::new(),
};

fn candidates_for(
    e: &Engine,
    goal: &Goal,
    locks: &Locks,
    db: &[(String, Vec<String>, String)],
    max: usize,
    seed: u64,
) -> Result<Vec<Change>, String> {
    let base = e.spec_rc();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9).max(1));
    let share = max / 4;
    let mut list: Vec<Change> = moves(&base, max, seed, locks).into_iter().map(Change::Tree).collect();
    let mut extra = if locks.swap_supports {
        support_changes(e, goal, share, &mut rng)
    } else {
        Vec::new()
    };
    extra.extend(item_changes(db, locks, share, &mut rng));
    if locks.tattoos {
        // Held back from the tree's share, so a round always has them when the build can take any.
        extra.extend(tattoo_changes(&base, share, &mut rng)?);
    }
    list.truncate(max.saturating_sub(extra.len()));
    list.extend(extra);
    Ok(list)
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
    pub change: String,
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
    /// Variants tried, by kind of change.
    pub tried_by_kind: BTreeMap<&'static str, usize>,
    pub calc_ms_median: f64,
}

/// One round: every change from `xml` on its own, on `threads` workers, each with its own engine (the engine is not `Send`). A tree change is undone by the next one writing its own tree;
/// anything else, or a change that took a config option off, gets a fresh engine.
fn round(
    xml: &str,
    list: &[Change],
    locks: &Locks,
    guard: &ConfigGuard,
    threads: usize,
    deadline: Instant,
) -> Vec<(Change, Result<Eval, String>, f64)> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(list.len()));
    std::thread::scope(|sc| {
        for _ in 0..threads.max(1) {
            sc.spawn(|| {
                let Ok(mut e) = Engine::load_xml(xml) else { return };
                let base = (*e.spec_rc()).clone();
                let mut mine = Vec::new();
                let mut dirty = false;
                let mut touched = false;
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= list.len() || Instant::now() >= deadline {
                        break;
                    }
                    let t = Instant::now();
                    // A tree change writes the whole tree, so it may follow another one; anything else must start from the build itself.
                    if dirty || (touched && !matches!(list[i], Change::Tree(_))) {
                        let Ok(fresh) = Engine::load_xml(xml) else { break };
                        e = fresh;
                    }
                    // A panic of the engine on one variant must not end the search; that variant is refused and its engine replaced.
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| try_change(&mut e, &base, &list[i], locks, guard)))
                        .unwrap_or_else(|_| Err("the engine panicked".into()));
                    touched = true;
                    dirty = !matches!(list[i], Change::Tree(_)) || !matches!(&r, Ok(v) if v.dropped_config.is_empty());
                    mine.push((list[i].clone(), r, t.elapsed().as_secs_f64() * 1000.0));
                }
                if let Ok(mut o) = out.lock() {
                    o.extend(mine);
                }
            });
        }
    });
    out.into_inner().unwrap_or_default()
}

/// Greedy search: each round takes the best allowed change and goes on from there, until the time is up or no change helps.
pub fn optimize(xml: &str, goal: &Goal, locks: &Locks, opt: Options) -> Result<Report, String> {
    let begin = Instant::now();
    let deadline = begin + std::time::Duration::from_secs_f64(opt.seconds);
    let load = Instant::now();
    let mut e = Engine::load_xml(xml)?;
    let load_ms = load.elapsed().as_secs_f64() * 1000.0;
    let start = evaluate(&e);
    // The options are judged against the build the user brought, for the whole search.
    let guard = ConfigGuard::new(&e);
    let db = if locks.item_slots.is_empty() { Vec::new() } else { uniques()? };
    let mut cur_xml = xml.to_string();
    let mut cur_score = 1.0;
    let mut best = start.clone();
    let mut steps = Vec::new();
    let mut rejected = BTreeMap::new();
    let (mut tried, mut accepted) = (0, 0);
    let mut times = Vec::new();
    let mut by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut n = 0u64;
    while Instant::now() < deadline {
        n += 1;
        let base = (*e.spec_rc()).clone();
        let mut rng = Rng(opt.seed.wrapping_add(n).wrapping_mul(0x9E37_79B9).max(1));
        let mut list = candidates_for(&e, goal, locks, &db, opt.moves_per_round, opt.seed.wrapping_add(n))?;
        rng.shuffle(&mut list);
        if list.is_empty() {
            break;
        }
        let results = round(&cur_xml, &list, locks, &guard, opt.threads, deadline);
        tried += results.len();
        let mut top: Option<(f64, Change, Eval)> = None;
        for (c, r, ms) in results {
            times.push(ms);
            let kind = match c {
                Change::Tree(_) => "tree",
                Change::Support { .. } => "gem",
                Change::Tattoo { .. } => "tattoo",
                Change::Item { .. } => "item",
            };
            *by_kind.entry(kind).or_insert(0) += 1;
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
                top = Some((s, c, v));
            }
        }
        let Some((s, c, v)) = top else { break };
        let mut fresh = Engine::load_xml(&cur_xml)?;
        let again = try_change(&mut fresh, &base, &c, locks, &guard)?;
        if again != v {
            return Err(format!("the same change gave different numbers on another engine: {c:?}"));
        }
        cur_xml = fresh.save_xml()?;
        // A fresh engine from the XML: what the user gets is what was scored.
        e = Engine::load_xml(&cur_xml)?;
        cur_score = s;
        let mut kept = evaluate(&e);
        if kept.dps != v.dps || kept.ehp != v.ehp {
            return Err(format!("the saved build scores differently from the variant: {c:?}"));
        }
        kept.dropped_config = v.dropped_config;
        best = kept;
        steps.push(Step {
            round: n as usize,
            tried,
            score: s,
            eval: best.clone(),
            change: c.describe(&base),
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
        tried_by_kind: by_kind,
        calc_ms_median: times.get(times.len() / 2).copied().unwrap_or(0.0),
    })
}
