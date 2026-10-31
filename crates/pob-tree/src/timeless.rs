//! Timeless jewels (R5.5c, MINEFIELD W6): the legion passives of the tree (PassiveTree.lua:79, :723-751) and the conquest half
//! of `PassiveSpec:BuildAllDependsAndPaths` (PassiveSpec.lua:1092-1406) -- which nodes a timeless jewel conquers (:1129-1203)
//! and what becomes of them (:1222-1406: `ReplaceNode` :1697, `NodeAdditionOrReplacementFromString` :2402,
//! `ReconnectNodeToClassStart` :1722), line for line. The LUTs are pob-data's (`pob_data::timeless::read_lut`).
//!
//! Out of scope: abyss conquests (jewel types 7-11, `data.readAbyssJewelLUT`/`resolveAbyssJewelComponent`, :1110-1127 and
//! :1253-1268) -- an explicit error; tattoos (`hashOverrides`, P5); Intuitive Leap-like bookkeeping of the radius pass (P5).
//!
//! D13 -- the `pairs` walks of the conquest:
//! * `pairs(self.jewels)` building `radiusJewels` (:1130): only decides which jewel wins a node two conquering jewels share; that
//!   case is recorded in `unimplemented` instead of being resolved silently.
//! * `pairs(additions)` (:1324, Glorious Vanity "Might/Legacy of the Vaal"): the additions are appended in this order, so it
//!   decides the order of `sd`/`mods`/`modList`. Ascending id is NOT it (the synthetic GV build: 4 of 23 notables differ), so the
//!   order is LuaJIT's own -- `crate::lj_table` models `lj_tab.c` for this table (corpus: `tests/timeless_pairs.rs`, 4 000/4 000)
//!   and `pob-calc/tests/timeless_luajit.rs` checks the GV build against LuaJIT.
//! * `pairs(addition.stats)` (:1327): no non-abyss addition has two stats (`tools/lua-corpus/export-legion.ts` survey: 0).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::timeless::{LegionStat, abyss_notable_names, legion_passives, read_lut, timeless_jewel_additions, timeless_jewel_type};
use pob_data::wire::OrderedMap;
use pob_lua::pattern::{Repl, gsub, match_};
use pob_lua::tostring_num;
use pob_mods::{Mod, ModType, Value, create_mod};

use crate::spec::PassiveSpec;
use crate::tree::{NodeStats, NodeType, PassiveTree, StatHolder, TreeError, process_stats};

const SPEC: &str = "Classes/PassiveSpec.lua";

fn err(line: u32, what: impl std::fmt::Display) -> TreeError {
    TreeError(format!("{SPEC}:{line}: {what}"))
}

/// A processed `legion.nodes[i]` (PassiveTree.lua:733-756: type, ProcessStats).
#[derive(Debug)]
pub struct LegionNode {
    /// e.g. `"vaal_keystone_1"` -- the mods are sourced `"Tree:" .. id`.
    pub id: String,
    pub dn: String,
    /// After ProcessStats (split in place).
    pub sd: Vec<String>,
    pub ty: NodeType,
    pub stats: NodeStats,
    pub sorted_stats: Vec<String>,
    pub stat_defs: &'static OrderedMap<LegionStat>,
}

/// `legion.additions[i]` (only `dn` is rewritten, :726-731).
#[derive(Debug)]
pub struct LegionAddition {
    pub id: String,
    pub dn: String,
    pub sd: Vec<String>,
    pub sorted_stats: Vec<String>,
    pub stats: &'static OrderedMap<LegionStat>,
}

/// `tree.legion`.
#[derive(Debug)]
pub struct Legion {
    /// `legion.nodes`, 0-based (upstream's `legionNodes[i + 1]`).
    pub nodes: Vec<Rc<LegionNode>>,
    pub additions: Vec<LegionAddition>,
    /// The legacy keystones `keystoneMap[node.dn] = node` added (:739-741): dn -> index into `nodes`. Only names the tree's own
    /// keystone map does not have.
    pub keystone_map: IndexMap<String, usize>,
}

impl PassiveTree {
    /// `self.legion` after PassiveTree.lua:726-756 (built on first use; nothing else reads the raw table).
    pub fn legion(&self) -> Result<&Legion, TreeError> {
        self.legion.get_or_init(|| build_legion(self)).as_ref().map_err(Clone::clone)
    }

    /// `self.keystoneMap[name]`'s modList: the tree's keystone, else a legacy legion keystone (:739-741).
    pub fn keystone_mods(&self, name: &str) -> Option<&[Rc<Mod>]> {
        if let Some(&i) = self.keystone_map.get(name) {
            return Some(&self.nodes[i].stats.mod_list.mods);
        }
        let legion = self.legion().ok()?;
        legion.keystone_map.get(name).map(|&i| &legion.nodes[i].stats.mod_list.mods[..])
    }
}

fn build_legion(tree: &PassiveTree) -> Result<Legion, TreeError> {
    let raw = legion_passives().map_err(|e| TreeError(e.0))?;
    let names = abyss_notable_names();
    // :726-731 -- abyss notables named "Notable N" take their first stat's manual name, or their first line.
    let additions = raw
        .additions
        .iter()
        .map(|a| {
            let numbered = |s: &str, p: &str| matches!(match_(s.as_bytes(), p.as_bytes(), None), Ok(Some(_)));
            let mut dn = a.dn.clone();
            if numbered(&a.id, "^abyss_.+_notable_%d+$") && numbered(&a.dn, "^Notable %d+$") {
                dn = a
                    .sorted_stats
                    .first()
                    .and_then(|k| names.get(k).cloned())
                    .or_else(|| a.sd.first().cloned())
                    .unwrap_or_default();
            }
            LegionAddition {
                id: a.id.clone(),
                dn,
                sd: a.sd.clone(),
                sorted_stats: a.sorted_stats.clone(),
                stats: &a.stats,
            }
        })
        .collect();
    let mut nodes = Vec::with_capacity(raw.nodes.len());
    let mut keystone_map = IndexMap::new();
    for (idx, n) in raw.nodes.iter().enumerate() {
        // :735-743 -- determine node type; legacy keystones join the keystone map unless the tree has the name.
        let ty = if n.m {
            NodeType::Mastery
        } else if n.ks {
            if !tree.keystone_map.contains_key(&n.dn) && !keystone_map.contains_key(&n.dn) {
                keystone_map.insert(n.dn.clone(), idx);
            }
            NodeType::Keystone
        } else if n.not_ {
            NodeType::Notable
        } else {
            NodeType::Normal
        };
        // :755 `self:ProcessStats(node)`
        let holder = StatHolder {
            id: 0.0,
            str_id: Some(&n.id),
            ty: Some(ty),
            dn: &n.dn,
        };
        let mut sd = n.sd.clone();
        let mut stats = NodeStats::default();
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        nodes.push(Rc::new(LegionNode {
            id: n.id.clone(),
            dn: n.dn.clone(),
            sd,
            ty,
            stats,
            sorted_stats: n.sorted_stats.clone(),
            stat_defs: &n.stats,
        }));
    }
    Ok(Legion {
        nodes,
        additions,
        keystone_map,
    })
}

/// `conquerorList[name].id` -- a number (`1`) or a string (`"2_v2"`).
#[derive(Clone, Debug, PartialEq)]
pub enum ConquerorId {
    Num(f64),
    Str(String),
}

impl ConquerorId {
    /// `.. id` in a Lua concatenation.
    pub fn concat_text(&self) -> String {
        match self {
            ConquerorId::Num(n) => tostring_num(*n),
            ConquerorId::Str(s) => s.clone(),
        }
    }
}

/// `jewelData.conqueredBy` (ModParser.lua:5830-5853): `{ id = seed, conqueror = conquerorList[name] }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConqueredBy {
    pub id: f64,
    pub conqueror_type: String,
    pub conqueror_id: ConquerorId,
}

impl ConqueredBy {
    /// From the item's `jewelData.conqueredBy` value; `Err` names what upstream would index (`conqueror` nil -- an unknown name).
    pub fn from_value(v: &Value) -> Result<Option<ConqueredBy>, TreeError> {
        if !v.truthy() {
            return Ok(None);
        }
        let conq = v.get("conqueror");
        let ty = conq.get("type");
        let (Some(id), Some(ty)) = (v.get("id").as_num(), ty.as_str()) else {
            return Err(err(
                1115,
                format!("attempt to index field 'conqueror' / bad conqueredBy {v:?} (an unknown conqueror name)"),
            ));
        };
        let cid = match conq.get("id") {
            Value::Num(n) => ConquerorId::Num(n),
            Value::Str(s) => ConquerorId::Str(s.to_string()),
            other => return Err(err(1287, format!("conqueror id {other:?}"))),
        };
        Ok(Some(ConqueredBy {
            id,
            conqueror_type: ty.to_owned(),
            conqueror_id: cid,
        }))
    }
}

/// What BuildAllDependsAndPaths reads off `self.build.itemsTab.items[itemId]` of a socketed jewel.
#[derive(Clone, Debug, Default)]
pub struct SpecJewel {
    /// `item.id`.
    pub id: Option<f64>,
    /// `item.jewelRadiusIndex` (1-based).
    pub jewel_radius_index: Option<f64>,
    /// `item.jewelData` exists.
    pub has_jewel_data: bool,
    /// `item.jewelData.limitDisabled`.
    pub limit_disabled: bool,
    /// `item.jewelData.conqueredBy` (one shared object, like the Lua table).
    pub conquered_by: Option<Rc<ConqueredBy>>,
}

/// :1093-1105 `timelessJewelTypeByConqueror`.
fn jewel_type_by_conqueror(ty: &str) -> Option<usize> {
    Some(match ty {
        "vaal" => 1,
        "karui" => 2,
        "maraketh" => 3,
        "templar" => 4,
        "eternal" => 5,
        "kalguur" => 6,
        "abyss_murderous" => 7,
        "abyss_searching" => 8,
        "abyss_hypnotic" => 9,
        "abyss_ghastly" => 10,
        "abyss_special" => 11,
        _ => return None,
    })
}

/// One `radiusJewels` entry (:1129-1140).
pub(crate) struct RadiusJewel {
    socket: usize,
    jewel: Rc<SpecJewel>,
}

/// Common.lua:709-715 `round(val, 1)`.
fn round1(v: f64) -> f64 {
    (v * 10f64.powi(1) + 0.5).floor() / 10f64.powi(1)
}

/// :1234-1251 `replaceHelperFunc(statToFix, statKey, statMod, value)`.
fn replace_helper(stat_to_fix: &str, stat_key: &str, stat_mod: &LegionStat, value: Option<f64>) -> Result<String, TreeError> {
    let has = |p: &str| stat_key.contains(p);
    let mut value = value;
    if stat_mod.fmt.as_deref() == Some("g") {
        // note the only one we actually care about is "Ritual of Flesh" life regen
        let v = value.ok_or_else(|| err(1237, "attempt to perform arithmetic on local 'value' (a nil value)"))?;
        value = Some(if has("per_minute") {
            round1(v / 60.0)
        } else if has("permyriad") {
            v / 100.0
        } else if has("_ms") {
            v / 1000.0
        } else {
            v
        });
    }
    let sub = |pat: String| -> Result<String, TreeError> {
        let v = value.ok_or_else(|| err(1246, "bad argument #2 to 'gsub' (string/function/table expected)"))?;
        let repl = tostring_num(v);
        let (out, _) = gsub(stat_to_fix.as_bytes(), pat.as_bytes(), Repl::Str(repl.as_bytes()), None)
            .map_err(|e| err(1246, format!("gsub: {}", e.0)))?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    };
    if stat_mod.min != stat_mod.max {
        return sub(format!("%({}%-{}%)", tostring_num(stat_mod.min), tostring_num(stat_mod.max)));
    }
    // `statMod.min ~= value` -- only true for might/legacy of the vaal which can combine stats
    if value.is_none_or(|v| stat_mod.min != v) {
        return sub(tostring_num(stat_mod.min));
    }
    Ok(stat_to_fix.to_owned()) // if it doesn't need to be changed
}

const ATTRIBUTES: [&str; 3] = ["Dexterity", "Intelligence", "Strength"];

impl PassiveSpec {
    /// The jewels' item data (pob-calc `post_load_spec` sets it before `PostLoad`, as the items tab is loaded before the tree,
    /// Build.lua:655-677): item id -> what BuildAllDependsAndPaths reads.
    pub fn set_jewel_items(&mut self, items: Vec<(f64, Rc<SpecJewel>)>) {
        self.jewel_items = items;
    }

    fn jewel_item(&self, item_id: f64) -> Option<&Rc<SpecJewel>> {
        self.jewel_items.iter().find(|(id, _)| *id == item_id).map(|(_, j)| j)
    }

    /// :1110-1140 -- the abyss conquests (out of scope: an explicit error) and the radius jewels (`pairs(self.jewels)`).
    pub(crate) fn gather_radius_jewels(&mut self) -> Result<Vec<RadiusJewel>, TreeError> {
        let mut out = Vec::new();
        for &(socket_id, item_id) in &self.jewels.0 {
            let Some(jewel) = self.jewel_item(item_id).cloned() else { continue };
            let allocated = crate::tree::as_id(socket_id).is_some_and(|s| self.alloc_nodes.contains_key(&s));
            if let Some(cb) = &jewel.conquered_by
                && jewel_type_by_conqueror(&cb.conqueror_type).is_some_and(|t| t >= 7)
                && allocated
                && !jewel.limit_disabled
            {
                return Err(TreeError(format!(
                    "unimplemented: {SPEC}:1110-1127 abyss conquest ({} jewel in socket {socket_id}; data.readAbyssJewelLUT)",
                    cb.conqueror_type
                )));
            }
            if jewel.jewel_radius_index.is_some() && allocated && jewel.has_jewel_data && !jewel.limit_disabled {
                let socket = crate::tree::as_id(socket_id)
                    .and_then(|s| self.node_idx(s))
                    .expect("an allocated node");
                out.push(RadiusJewel { socket, jewel });
            }
        }
        // D13: two conquering jewels over one node -- the winner depends on `pairs(self.jewels)`.
        let conquering: Vec<&RadiusJewel> = out
            .iter()
            .filter(|r| r.jewel.id != Some(0.0) && r.jewel.conquered_by.is_some())
            .collect();
        if conquering.len() > 1 {
            self.unimplemented.push(format!(
                "D13 {SPEC}:1130: {} conquering timeless jewels -- a shared node goes to the last in pairs(self.jewels) order",
                conquering.len()
            ));
        }
        Ok(out)
    }

    /// :1169-1203 -- the `node.conqueredBy` a radius jewel gives node `i` (the Intuitive Leap/Impossible Escape half is P5).
    pub(crate) fn radius_conquest(&self, i: usize, radius_jewels: &[RadiusJewel]) -> Option<Rc<ConqueredBy>> {
        let node = &self.nodes[i];
        let ty = node.ty();
        if radius_jewels.is_empty() || ty == NodeType::ClassStart || ty == NodeType::Socket || node.ascendancy_name().is_some() {
            return None;
        }
        let mut conquered = None;
        for rj in radius_jewels {
            let radius_index = rj.jewel.jewel_radius_index.and_then(crate::tree::as_id);
            let in_radius = radius_index
                .and_then(|r| (r as usize).checked_sub(1))
                .and_then(|r| self.nodes[rj.socket].nodes_in_radius().and_then(|l| l.get(r)))
                .is_some_and(|l| l.binary_search(&node.id).is_ok());
            if in_radius
                && rj.jewel.id != Some(0.0)
                && let Some(cb) = &rj.jewel.conquered_by
                && jewel_type_by_conqueror(&cb.conqueror_type).is_none_or(|t| t < 7)
            {
                conquered = Some(cb.clone());
            }
        }
        conquered
    }

    /// :1222-1406 for one conquered node (not a socket), then `ReconnectNodeToClassStart` (:1406).
    pub(crate) fn conquer_node(&mut self, i: usize) -> Result<(), TreeError> {
        let tree = self.tree.clone();
        let legion = tree.legion()?;
        let cb = self.nodes[i].conquered_by.clone().expect("a conquered node");
        let node_id = self.nodes[i].id;
        let ty = self.nodes[i].ty();
        let additions_base = timeless_jewel_additions();
        let jewel_type = jewel_type_by_conqueror(&cb.conqueror_type).unwrap_or(5);
        let mut seed = cb.id;
        if jewel_type == 5 {
            seed /= 20.0;
        }
        let legion_node = |n: f64, line: u32| -> Result<Rc<LegionNode>, TreeError> {
            // `legionNodes[n]` (1-based); nil -> ReplaceNode indexes nil.
            (n >= 1.0 && n.fract() == 0.0)
                .then(|| legion.nodes.get(n as usize - 1).cloned())
                .flatten()
                .ok_or_else(|| err(line, format!("attempt to index a nil value (legionNodes[{}])", tostring_num(n))))
        };
        // :1271-1276 / :1367-1372 -- the seed range check, then the LUT.
        let lut = |line: u32| -> Result<Vec<f64>, TreeError> {
            let (_, mn, mx) = timeless_jewel_type(jewel_type).ok_or_else(|| err(line, format!("no timeless jewel type {jewel_type}")))?;
            if seed != pob_lua::number::max2(pob_lua::number::min2(seed, mx), mn) {
                // ConPrintf("ERROR: Seed .. is outside of valid range ..")
                return Ok(Vec::new());
            }
            read_lut(cb.id, node_id, jewel_type)
                .map(|v| v.into_iter().map(f64::from).collect())
                .map_err(|e| TreeError(e.0))
        };
        if jewel_type >= 7 && ty != NodeType::Mastery {
            return Err(err(1253, "unimplemented: abyss conquest (data.resolveAbyssJewelComponent)"));
        } else if ty == NodeType::Notable {
            let tbl = lut(1275)?;
            if tbl.is_empty() {
                // :1279 ConPrintf("Missing LUT: ..")
            } else if jewel_type == 1 {
                let header_size = tbl.len();
                if header_size == 2 || header_size == 3 {
                    let ln = legion_node(tbl[0] + 1.0 - additions_base, 1286)?;
                    self.replace_with_legion(i, &ln);
                    for (k, rep_stat) in ln.sd.iter().enumerate() {
                        let stat_key = ln.sorted_stats.get(k).map(String::as_str);
                        let stat_mod = stat_key
                            .and_then(|key| ln.stat_defs.get(key))
                            .ok_or_else(|| err(1291, "attempt to index local 'statMod' (a nil value)"))?;
                        let value = lua_index(&tbl, stat_mod.index + 1.0);
                        let rep = replace_helper(rep_stat, stat_key.unwrap_or_default(), stat_mod, value)?;
                        self.node_addition_or_replacement(i, &rep, k == 0); // wipe mods on first run
                    }
                } else if header_size == 6 || header_size == 8 {
                    let half = header_size / 2;
                    let mut bias = 0i32;
                    for &val in &tbl[..half] {
                        if val <= 21.0 {
                            bias += 1;
                        } else {
                            bias -= 1;
                        }
                    }
                    // :1306-1310 legionNodes[77] might of the vaal / [78] legacy of the vaal
                    let ln = legion_node(if bias >= 0 { 77.0 } else { 78.0 }, 1307)?;
                    self.replace_with_legion(i, &ln);
                    let mut additions: IndexMap<u64, f64> = IndexMap::new();
                    for k in 0..half {
                        let (val, roll) = (tbl[k], tbl[k + half]);
                        let key = val as u64;
                        match additions.get_mut(&key) {
                            None => {
                                additions.insert(key, roll);
                            }
                            Some(v) => *v += roll,
                        }
                    }
                    // D13 `pairs(additions)` -- LuaJIT's own order for this table (see the module comment).
                    let order = crate::lj_table::pairs_order(&tbl[..half]);
                    for add in order {
                        let add = add as u64;
                        let val = additions[&add];
                        let addition = legion
                            .additions
                            .get(add as usize)
                            .ok_or_else(|| err(1326, format!("attempt to index local 'addition' (legionAdditions[{}])", add + 1)))?;
                        for add_stat in &addition.sd {
                            let mut add_stat = add_stat.clone();
                            for (k, stat_mod) in addition.stats.iter() {
                                add_stat = replace_helper(&add_stat, k, stat_mod, Some(val))?;
                            }
                            self.node_addition_or_replacement(i, &add_stat, false);
                        }
                    }
                } else {
                    // :1334 ConPrintf("Unhandled Glorious Vanity headerSize: ..")
                }
            } else {
                for &jewel_data in &tbl {
                    if jewel_data >= additions_base {
                        // replace
                        let n = jewel_data + 1.0 - additions_base;
                        // else ConPrintf("Unhandled 'replace' ID: ..")
                        if let Some(ln) = legion.nodes.get(n as usize - 1).cloned() {
                            self.replace_with_legion(i, &ln);
                        }
                    } else {
                        // add
                        let addition = legion.additions.get(jewel_data as usize).ok_or_else(|| {
                            err(
                                1348,
                                format!("attempt to index local 'addition' (legionAdditions[{}])", jewel_data + 1.0),
                            )
                        })?;
                        for add_stat in &addition.sd {
                            self.node_addition_or_replacement(i, &format!(" \n{add_stat}"), false);
                        }
                    }
                }
            }
        } else if ty == NodeType::Keystone {
            let match_str = format!("{}_keystone_{}", cb.conqueror_type, cb.conqueror_id.concat_text());
            if let Some(ln) = legion.nodes.iter().find(|n| n.id == match_str).cloned() {
                self.replace_with_legion(i, &ln);
            }
        } else if ty == NodeType::Normal {
            let attribute = ATTRIBUTES.contains(&self.nodes[i].dn()); // `or node.isTattoo` -- tattoos are P5 (nil)
            match cb.conqueror_type.as_str() {
                "vaal" => {
                    let tbl = lut(1371)?;
                    if tbl.is_empty() {
                        // :1375 ConPrintf("Missing LUT: ..")
                    } else {
                        let ln = legion_node(tbl[0] + 1.0 - additions_base, 1377)?;
                        self.replace_with_legion(i, &ln);
                        // `ipairs(node.sd)` -- the legion node's table (each replacement swaps node.sd, not this one)
                        for (k, rep_stat) in ln.sd.iter().enumerate() {
                            let stat_key = ln.sorted_stats.get(k).map(String::as_str);
                            let stat_mod = stat_key
                                .and_then(|key| ln.stat_defs.get(key))
                                .ok_or_else(|| err(1381, "attempt to index local 'statMod' (a nil value)"))?;
                            let rep = replace_helper(rep_stat, stat_key.unwrap_or_default(), stat_mod, lua_index(&tbl, 2.0))?;
                            self.node_addition_or_replacement(i, &rep, true);
                        }
                    }
                }
                "karui" => {
                    let s = if attribute { "2" } else { "4" };
                    self.node_addition_or_replacement(i, &format!(" \n+{s} to Strength"), false);
                }
                "maraketh" => {
                    let s = if attribute { "2" } else { "4" };
                    self.node_addition_or_replacement(i, &format!(" \n+{s} to Dexterity"), false);
                }
                "kalguur" => {
                    let s = if attribute { "1" } else { "2" };
                    self.node_addition_or_replacement(i, &format!(" \n{s}% increased Ward"), false);
                }
                "templar" => {
                    if attribute {
                        let ln = legion_node(91.0, 1397)?; // templar_devotion_node
                        self.replace_with_legion(i, &ln);
                    } else {
                        self.node_addition_or_replacement(i, " \n+5 to Devotion", false);
                    }
                }
                "eternal" => {
                    let ln = legion_node(110.0, 1403)?; // eternal_small_blank
                    self.replace_with_legion(i, &ln);
                }
                _ => {}
            }
        }
        self.reconnect_node_to_class_start(i)
    }

    /// :1697-1720 `ReplaceNode(old, legionNode)`.
    fn replace_with_legion(&mut self, i: usize, ln: &Rc<LegionNode>) {
        let node = &mut self.nodes[i];
        // `if old.sd == newNode.sd then return 1 end`
        if node.own_sd.is_none() && node.replaced.as_ref().is_some_and(|r| Rc::ptr_eq(r, ln)) {
            return;
        }
        node.replaced = Some(ln.clone());
        node.own_sd_effect = None;
        node.own_sd = None;
        // `old.modList = new("ModList"); AddList(newNode.modList)` -- the same mod objects (read through `replaced`).
        node.own_stats = None;
        // `newNode.reminderText or wipeTable(old.reminderText)` -- legion nodes have none.
        node.own_reminder_text = Some(Vec::new());
    }

    /// :2402-2482 `NodeAdditionOrReplacementFromString(node, sd, replacement)`.
    fn node_addition_or_replacement(&mut self, i: usize, sd: &str, replacement: bool) {
        let mode = self.tree.parse_mode;
        let node_id = self.nodes[i].id;
        let mut add_sd = vec![sd.to_owned()];
        let mut add = NodeStats::default();
        // The addition's own `unknown`/`extra` (:2443-2447) are never copied to the node.
        let holder = StatHolder {
            id: f64::from(node_id),
            str_id: None,
            ty: None,
            dn: "",
        };
        process_stats(&holder, &mut add_sd, &mut add, 1, mode);
        let node = &mut self.nodes[i];
        let cur = node.stats().clone();
        let (new_sd, new_stats) = if replacement {
            (
                add_sd,
                NodeStats {
                    mods: add.mods,
                    mod_key: add.mod_key,
                    mod_list: add.mod_list,
                    keystone_mod: cur.keystone_mod,
                    unknown: cur.unknown,
                    extra: cur.extra,
                },
            )
        } else {
            let mut sd_all = node.sd().to_vec();
            sd_all.extend(add_sd);
            let mut mods = cur.mods;
            mods.extend(add.mods);
            // `modList:AddList(addition.modList); modList:AddList(node.modList)` -- the addition first.
            let mut list = add.mod_list;
            list.mods.extend(cur.mod_list.mods);
            (
                sd_all,
                NodeStats {
                    mods,
                    mod_key: cur.mod_key + &add.mod_key,
                    mod_list: list,
                    keystone_mod: cur.keystone_mod,
                    unknown: cur.unknown,
                    extra: cur.extra,
                },
            )
        };
        node.own_sd_effect = None;
        node.own_sd = Some(new_sd);
        node.own_stats = Some(new_stats);
    }

    /// :1722-1730 `ReconnectNodeToClassStart(node)` -- a conquered Normal node next to a class start gets its
    /// `Condition:ConnectedTo<Class>Start` back.
    fn reconnect_node_to_class_start(&mut self, i: usize) -> Result<(), TreeError> {
        let tree = self.tree.clone();
        let linked = self.nodes[i].tree_node().linked_id.clone();
        for linked_id in linked {
            for class in &tree.classes {
                if class.start_node_id == Some(linked_id) && self.nodes[i].ty() == NodeType::Normal {
                    let node = &mut self.nodes[i];
                    if node.own_stats.is_none() {
                        if node.replaced.is_none() {
                            // The node still holds the TREE's modList: NewMod would write into the shared tree node.
                            return Err(err(
                                1726,
                                format!(
                                    "unimplemented: NewMod into the tree's own modList of node {} (conquest left it unchanged)",
                                    node.id
                                ),
                            ));
                        }
                        node.own_stats = Some(node.stats().clone());
                    }
                    let st = node.own_stats.as_mut().expect("set above");
                    st.mod_list.mods.push(Rc::new(create_mod(
                        &format!("Condition:ConnectedTo{}Start", class.name),
                        Some(ModType::Flag),
                        Value::Bool(true),
                        Some(&format!("Tree:{linked_id}")),
                        0,
                        0,
                        Vec::new(),
                    )));
                }
            }
        }
        Ok(())
    }
}

/// `t[k]` of a Lua array (1-based) with a number key; nil outside.
fn lua_index(t: &[f64], k: f64) -> Option<f64> {
    (k >= 1.0 && k.fract() == 0.0).then(|| t.get(k as usize - 1).copied()).flatten()
}
