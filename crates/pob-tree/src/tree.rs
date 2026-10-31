//! `Classes/PassiveTree.lua` -- the calc-relevant half (pob-ts `src/tree/tree.ts`): node migration, node types, the name
//! maps, orbit geometry and `ProcessStats` (stat lines -> `node.modList`). Sprites, assets, connectors, jewel radii,
//! legion/tattoo subgraphs and `buildTreeDependentUniques` are P5 and are listed in `unimplemented` rather than dropped
//! silently (CALC-ARCH §3, §11).
//!
//! D13: upstream walks `pairs(self.nodes)` / `pairs(self.classes)` in hash order. Every loop here goes in ascending node id
//! / ascending classId, as pob-ts does; the two places where the order could change a RESULT (`notableMap`'s duplicate-name
//! rule, `group.ascendancyName`) are called out where they happen.
//!
//! The tree is built once per version and thread (`load_passive_tree`, = `main:LoadTree`): its mods hold `Rc`s, so it is
//! not `Send`, and the mod cache it parses from is per thread too.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::tree::{NodeRef, TreeExpansionJewel, TreeJson, TreeNode as RawNode, load_tree, tree_version_num};
use pob_data::wire::LuaTable;
use pob_lua::pattern::gmatch;
use pob_lua::string::lower;
use pob_lua::tostring_num;
use pob_mods::{Mod, ModList, ModType, Value, create_mod};
use pob_parser::parse_mod_line;

use crate::modtools::{format_mod, set_source};

/// A Lua error / missing data while building the tree or a spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeError(pub String);

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TreeError {}

impl From<pob_data::DataError> for TreeError {
    fn from(e: pob_data::DataError) -> Self {
        TreeError(e.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeType {
    ClassStart,
    AscendClassStart,
    Mastery,
    Socket,
    Keystone,
    Notable,
    Normal,
}

impl NodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::ClassStart => "ClassStart",
            NodeType::AscendClassStart => "AscendClassStart",
            NodeType::Mastery => "Mastery",
            NodeType::Socket => "Socket",
            NodeType::Keystone => "Keystone",
            NodeType::Notable => "Notable",
            NodeType::Normal => "Normal",
        }
    }

    /// `type == "ClassStart" or type == "AscendClassStart"`.
    pub fn is_start(self) -> bool {
        matches!(self, NodeType::ClassStart | NodeType::AscendClassStart)
    }
}

/// PassiveTree.lua:851 `node.mods[i] = { list = list, extra = extra }`; `combined` marks a line swallowed by the line above
/// (:829). The mods in `list` are the same objects as in `modList` (`setSource` mutates in place, :864).
#[derive(Clone, Debug, Default)]
pub struct NodeModLine {
    pub list: Option<Vec<Rc<Mod>>>,
    pub extra: Option<String>,
    pub combined: bool,
}

/// What `ProcessStats` writes onto its node: `mods`, `modKey`, `modList`, the `unknown`/`extra` markers (written only when
/// true, :845/:848) and `keystoneMod`.
#[derive(Clone, Debug, Default)]
pub struct NodeStats {
    /// 1-based upstream; entry `i` here is `node.mods[i + 1]`. `None` = a hole (never left by ProcessStats itself).
    pub mods: Vec<Option<NodeModLine>>,
    pub mod_key: String,
    pub mod_list: ModList,
    pub unknown: bool,
    pub extra: bool,
    pub keystone_mod: Option<Rc<Mod>>,
}

/// `self.groups[g]` with the two fields node processing writes.
#[derive(Clone, Debug)]
pub struct TreeGroup {
    pub x: f64,
    pub y: f64,
    pub is_proxy: bool,
    /// Written by the last node of the group upstream sees; here by the highest-id one (D13). Display only.
    pub ascendancy_name: Option<String>,
    pub is_ascendancy_start: bool,
}

/// A tree node after migration (:522-541) and processing.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: u32,
    pub dn: String,
    /// Modern trees only (`allocatedMasteryTypes` keys off it, PassiveSpec.lua:1433).
    pub name: Option<String>,
    /// After `ProcessStats` split multi-line stats in place (:805-813).
    pub sd: Vec<String>,
    pub ty: NodeType,
    /// The Lua group key as the data holds it (`node.g`).
    pub g: Option<i64>,
    pub o: Option<f64>,
    pub oidx: Option<f64>,
    /// `node.group` -- the key of a group that exists (`self.groups[node.g]`).
    pub group: Option<i64>,
    pub ascendancy_name: Option<String>,
    pub class_start_index: Option<f64>,
    pub is_ascendancy_start: bool,
    pub is_multiple_choice: bool,
    pub is_multiple_choice_option: bool,
    pub is_proxy: bool,
    pub is_blighted: bool,
    pub is_jewel_socket: bool,
    pub is_mastery: bool,
    pub expansion_jewel: Option<TreeExpansionJewel>,
    pub passive_points_granted: f64,
    pub granted_strength: Option<f64>,
    pub granted_dexterity: Option<f64>,
    pub granted_intelligence: Option<f64>,
    pub reminder_text: Option<Vec<String>>,
    /// The `effect` ids of `node.masteryEffects` (their stats live in `PassiveTree::mastery_effects`).
    pub mastery_effect_ids: Vec<f64>,
    pub icon: String,
    /// Every `out`, then every `in` (:622-650) -- the order `linked` and the path search depend on.
    pub linked_id: Vec<u32>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub angle: Option<f64>,
    pub stats: NodeStats,
}

/// `self.masteryEffects[effect] = { id = effect, sd = effect.stats }` (:562), processed by `ProcessStats`.
#[derive(Clone, Debug)]
pub struct MasteryEffect {
    pub id: f64,
    pub sd: Vec<String>,
    pub stats: NodeStats,
}

#[derive(Clone, Debug)]
pub struct AscendClass {
    pub id: Option<String>,
    pub name: String,
    pub start_node_id: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct TreeClass {
    pub name: String,
    pub base_str: Option<f64>,
    pub base_dex: Option<f64>,
    pub base_int: Option<f64>,
    /// Index = ascendClassId; `[0]` is always `{ name = "None" }` (:181).
    pub classes: Vec<AscendClass>,
    pub start_node_id: Option<u32>,
}

/// One `ascendNameMap` entry: `{ classId, class, ascendClassId, ascendClass }` as indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AscendEntry {
    pub class_id: usize,
    pub ascend_class_id: usize,
}

pub struct PassiveTree {
    pub tree_version: String,
    pub version_num: f64,
    pub json: &'static TreeJson,
    /// Every node except `root` (:510), ascending id.
    pub nodes: Vec<TreeNode>,
    node_index: HashMap<u32, usize>,
    pub groups: HashMap<i64, TreeGroup>,
    /// Index = classId (0 Scion .. 6 Shadow) after the 1 -> 0 migration (:117-122, D12).
    pub classes: Vec<TreeClass>,
    pub class_name_map: IndexMap<String, usize>,
    pub ascend_name_map: IndexMap<String, AscendEntry>,
    /// Name -> index into `nodes`; keystones under `dn` and `dn:lower()` (:577-578).
    pub keystone_map: IndexMap<String, usize>,
    pub notable_map: IndexMap<String, usize>,
    pub ascendancy_map: IndexMap<String, usize>,
    pub cluster_node_map: IndexMap<String, usize>,
    pub class_notables: IndexMap<String, Vec<String>>,
    pub sockets: IndexMap<u32, usize>,
    pub mastery_effects: IndexMap<u64, MasteryEffect>,
    pub skills_per_orbit: Vec<f64>,
    pub orbit_radii: Vec<f64>,
    /// 0-based: entry `o` is upstream's `orbitAnglesByOrbit[o + 1]` (L8).
    pub orbit_angles_by_orbit: Vec<Vec<f64>>,
    /// Upstream branches this crate does not port; read by reports, never by the calc.
    pub unimplemented: Vec<String>,
}

const LEGACY_SKILLS_PER_ORBIT: [f64; 5] = [1.0, 6.0, 12.0, 12.0, 40.0];
const LEGACY_ORBIT_RADII: [f64; 5] = [0.0, 82.0, 162.0, 335.0, 493.0];

/// An integral, non-negative Lua number that fits a node id.
pub(crate) fn as_id(n: f64) -> Option<u32> {
    (n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX)).then_some(n as u32)
}

fn as_index(n: Option<f64>) -> Option<usize> {
    n.and_then(as_id).map(|i| i as usize)
}

/// `math.rad`: LuaJIT multiplies by the constant `pi/180`.
fn m_rad(deg: f64) -> f64 {
    deg * (std::f64::consts::PI / 180.0)
}

/// PassiveTree.lua:1041-1062 (0-based: entry `i` is upstream's `orbitAngles[i + 1]`).
pub fn calc_orbit_angles(nodes_in_orbit: f64) -> Vec<f64> {
    let degrees: Vec<f64> = if nodes_in_orbit == 16.0 {
        vec![
            0., 30., 45., 60., 90., 120., 135., 150., 180., 210., 225., 240., 270., 300., 315., 330.,
        ]
    } else if nodes_in_orbit == 40.0 {
        vec![
            0., 10., 20., 30., 40., 45., 50., 60., 70., 80., 90., 100., 110., 120., 130., 135., 140., 150., 160., 170., 180., 190., 200.,
            210., 220., 225., 230., 240., 250., 260., 270., 280., 290., 300., 310., 315., 320., 330., 340., 350.,
        ]
    } else {
        // `for i = 0, nodesInOrbit` -- inclusive, so one longer than the orbit.
        let mut v = Vec::new();
        let mut i = 0.0;
        while i <= nodes_in_orbit {
            v.push(360.0 * i / nodes_in_orbit);
            i += 1.0;
        }
        v
    };
    degrees.into_iter().map(m_rad).collect()
}

/// `tonumber(otherId)` of an `in`/`out` entry; a non-numeric one is `nil`, and `t_insert(t, nil)` is a no-op.
fn node_ref_id(r: &NodeRef) -> Option<u32> {
    match r {
        NodeRef::Num(n) => as_id(*n),
        NodeRef::Str(s) => pob_lua::tonumber(s).and_then(as_id),
    }
}

/// The parts of `ProcessStats` that only need the holder's identity.
pub(crate) struct StatHolder<'a> {
    pub id: f64,
    pub ty: Option<NodeType>,
    pub dn: &'a str,
}

/// PassiveTree.lua:795-873 `ProcessStats(node, startIndex)`, line for line. `sd` is the node's stat table (split in place);
/// `stats` its `mods`/`modKey`/`modList`/markers. `parseMod` is the ModCache-backed `pob_parser::parse_mod_line`: a miss
/// reads exactly like a line the parser cannot understand (`unimplemented`: ModParser.lua:6994 real parser, R5).
pub(crate) fn process_stats(holder: &StatHolder<'_>, sd: &mut Vec<String>, stats: &mut NodeStats, start_index: usize) {
    if start_index == 1 {
        stats.mod_key = String::new();
        stats.mods = Vec::new();
        stats.mod_list = ModList::default();
    }
    // `if not node.sd then return end` -- every holder here has a table.
    let set_mod = |mods: &mut Vec<Option<NodeModLine>>, i: usize, v: NodeModLine| {
        if mods.len() < i {
            mods.resize(i, None);
        }
        mods[i - 1] = Some(v);
    };
    let mut i = start_index;
    while i <= sd.len() {
        if sd[i - 1].contains('\n') {
            // :805-813 -- split in place, in front of the remaining lines.
            let line = sd.remove(i - 1);
            for (il, caps) in (i..).zip(gmatch(line.as_bytes(), b"[^\n]+")) {
                let caps = caps.expect("[^\\n]+ is a valid pattern");
                let part = caps.get(0).and_then(|c| c.bytes(line.as_bytes())).expect("whole-match capture");
                sd.insert(il - 1, String::from_utf8_lossy(part).into_owned());
            }
            if i > sd.len() {
                unimplemented!("Classes/PassiveTree.lua:815 parseMod(nil) after splitting an all-newline stat (a Lua error)");
            }
        }
        let line = sd[i - 1].clone();
        let mut parsed = parse_mod_line(&line, false);
        if parsed.mod_list.is_none() || parsed.extra.is_some() {
            // :816-834 -- try to combine with one or more of the following lines.
            let mut end_i = i + 1;
            while end_i <= sd.len() {
                let mut comb = line.clone();
                for l in &sd[i..end_i] {
                    comb.push(' ');
                    comb.push_str(l);
                }
                parsed = parse_mod_line(&comb, true);
                if parsed.mod_list.is_some() && parsed.extra.is_none() {
                    for ci in i + 1..=end_i {
                        set_mod(
                            &mut stats.mods,
                            ci,
                            NodeModLine {
                                list: Some(Vec::new()),
                                extra: None,
                                combined: true,
                            },
                        );
                    }
                    break;
                }
                end_i += 1;
            }
        }
        let list: Option<Vec<Rc<Mod>>> = parsed.mod_list.map(|l| l.into_iter().map(Rc::new).collect());
        match (&list, &parsed.extra) {
            (None, _) => stats.unknown = true,
            (Some(_), Some(_)) => stats.extra = true,
            (Some(l), None) => {
                for m in l {
                    stats.mod_key.push('[');
                    stats.mod_key.push_str(&format_mod(m));
                    stats.mod_key.push(']');
                }
            }
        }
        set_mod(
            &mut stats.mods,
            i,
            NodeModLine {
                list,
                extra: parsed.extra,
                combined: false,
            },
        );
        i += 1;
        while stats.mods.get(i - 1).is_some_and(Option::is_some) {
            // Skip the lines the combining code filled with dummy lists.
            i += 1;
        }
    }
    // :860-869 -- one list from every recognised line, sourced to this node. `#node.mods` = len (no holes).
    let source = format!("Tree:{}", tostring_num(holder.id));
    for idx in start_index..=stats.mods.len() {
        let Some(entry) = stats.mods[idx - 1].as_mut() else {
            continue;
        };
        if entry.extra.is_some() {
            continue;
        }
        if let Some(list) = entry.list.as_mut() {
            for m in list.iter_mut() {
                // The mods of lines >= startIndex were made by this call: `get_mut` cannot fail.
                set_source(Rc::get_mut(m).expect("a fresh parseMod copy"), &source);
                stats.mod_list.mods.push(m.clone());
            }
        }
    }
    if holder.ty == Some(NodeType::Keystone) {
        // :871 -- "Tree"..id, with no colon. Verbatim, bug and all.
        stats.keystone_mod = Some(Rc::new(create_mod(
            "Keystone",
            Some(ModType::List),
            Value::str(holder.dn),
            Some(&format!("Tree{}", tostring_num(holder.id))),
            0,
            0,
            Vec::new(),
        )));
    }
}

thread_local! {
    static TREES: RefCell<HashMap<String, Rc<PassiveTree>>> = RefCell::new(HashMap::new());
}

/// `main:LoadTree(treeVersion)` -- built once per version (per thread) and shared, like the desktop.
pub fn load_passive_tree(version: &str) -> Result<Rc<PassiveTree>, TreeError> {
    if let Some(t) = TREES.with(|c| c.borrow().get(version).cloned()) {
        return Ok(t);
    }
    let tree = Rc::new(PassiveTree::build(version)?);
    TREES.with(|c| c.borrow_mut().insert(version.to_owned(), tree.clone()));
    Ok(tree)
}

impl PassiveTree {
    /// `PassiveTreeClass:PassiveTree(treeVersion)` without the cache (benchmarks build it repeatedly).
    pub fn build(version: &str) -> Result<PassiveTree, TreeError> {
        let json = load_tree(version)?;
        let version_num = tree_version_num(version)?;
        let skills_per_orbit = json
            .constants
            .skills_per_orbit
            .clone()
            .unwrap_or_else(|| LEGACY_SKILLS_PER_ORBIT.to_vec());
        let orbit_radii = json.constants.orbit_radii.clone().unwrap_or_else(|| LEGACY_ORBIT_RADII.to_vec());
        let mut t = PassiveTree {
            tree_version: version.to_owned(),
            version_num,
            json,
            nodes: Vec::new(),
            node_index: HashMap::new(),
            groups: HashMap::new(),
            classes: Vec::new(),
            class_name_map: IndexMap::new(),
            ascend_name_map: IndexMap::new(),
            keystone_map: IndexMap::new(),
            notable_map: IndexMap::new(),
            ascendancy_map: IndexMap::new(),
            cluster_node_map: IndexMap::new(),
            class_notables: IndexMap::new(),
            sockets: IndexMap::new(),
            mastery_effects: IndexMap::new(),
            orbit_angles_by_orbit: skills_per_orbit.iter().map(|&n| calc_orbit_angles(n)).collect(),
            skills_per_orbit,
            orbit_radii,
            unimplemented: vec![
                "PassiveTree.lua:79-80 legion/tattoo passives (P5)".into(),
                "PassiveTree.lua:131-172 alternate_ascendancies / secondaryAscendNameMap (P5)".into(),
                "PassiveTree.lua:232-500 assets, sprites, node overlays (display only)".into(),
                "PassiveTree.lua:623-640 connector polygons (display only)".into(),
                "PassiveTree.lua:651-711 socket/keystone nodesInRadius (jewels, P5)".into(),
                "PassiveTree.lua:723-787 legion additions / tattoo nodes (P5)".into(),
                "PassiveTree.lua:790 buildTreeDependentUniques (P5)".into(),
            ],
        };
        t.build_classes()?;
        t.build_nodes()?;
        t.build_links();
        t.add_class_start_conditions();
        Ok(t)
    }

    /// `self.nodes[id]`.
    pub fn node(&self, id: u32) -> Option<&TreeNode> {
        self.node_index.get(&id).map(|&i| &self.nodes[i])
    }

    pub(crate) fn node_idx(&self, id: u32) -> Option<usize> {
        self.node_index.get(&id).copied()
    }

    /// `self.masteryEffects[effect]`.
    pub fn mastery_effect(&self, effect: f64) -> Option<&MasteryEffect> {
        as_id(effect).and_then(|e| self.mastery_effects.get(&u64::from(e)))
    }

    /// :117-122 + :174-205: the 1 -> 0 migration (already in the wire form, D12/D16), `class.classes`, `[0] = None`, the maps.
    fn build_classes(&mut self) -> Result<(), TreeError> {
        let modern = self.version_num >= 3.1;
        let entries: Vec<(i64, &pob_data::tree::TreeClass)> = match &self.json.classes {
            // A modern tree is a Lua 1..7 sequence moved to 0..6: the JSON array index IS the classId.
            LuaTable::Array(v) => v.iter().enumerate().map(|(i, c)| (i as i64, c)).collect(),
            LuaTable::Object(_) => self
                .json
                .classes
                .entries()
                .into_iter()
                .map(|(k, c)| {
                    k.map(|k| (k, c))
                        .ok_or_else(|| TreeError("PassiveTree: non-numeric class key".into()))
                })
                .collect::<Result<_, _>>()?,
        };
        for (expect, (class_id, raw)) in entries.into_iter().enumerate() {
            if class_id != expect as i64 {
                return Err(TreeError(format!(
                    "PassiveTree: class ids are not 0..n (got {class_id} at {expect})"
                )));
            }
            let mut classes = vec![AscendClass {
                id: None,
                name: "None".into(),
                start_node_id: None,
            }];
            let push = |classes: &mut Vec<AscendClass>, a: &pob_data::tree::TreeAscendancy| {
                classes.push(AscendClass {
                    id: a.id.clone(),
                    name: a.name.clone(),
                    start_node_id: None,
                })
            };
            if modern {
                for a in raw.ascendancies.iter().flatten() {
                    push(&mut classes, a);
                }
            } else if let Some(list) = &raw.classes {
                for (k, a) in list.entries() {
                    if k != Some(classes.len() as i64) {
                        return Err(TreeError(format!("PassiveTree: {} ascendancy keys are not 1..n", raw.name)));
                    }
                    push(&mut classes, a);
                }
            }
            self.class_name_map.insert(raw.name.clone(), self.classes.len());
            self.classes.push(TreeClass {
                name: raw.name.clone(),
                base_str: raw.base_str,
                base_dex: raw.base_dex,
                base_int: raw.base_int,
                classes,
                start_node_id: None,
            });
        }
        // `pairs(class.classes)` includes index 0, so "None" lands in ascendNameMap too; ascending classId makes the highest
        // classId win it (D13). No path looks "None" up.
        for (class_id, cls) in self.classes.iter().enumerate() {
            for (ascend_class_id, a) in cls.classes.iter().enumerate() {
                let entry = AscendEntry { class_id, ascend_class_id };
                if let Some(id) = &a.id {
                    self.ascend_name_map.insert(id.clone(), entry);
                }
                self.ascend_name_map.insert(a.name.clone(), entry);
            }
        }
        Ok(())
    }

    /// :513-620 -- one pass over every node, ascending id (D13).
    fn build_nodes(&mut self) -> Result<(), TreeError> {
        let json = self.json;
        for (k, g) in json.groups.entries() {
            let Some(k) = k else { continue };
            self.groups.insert(
                k,
                TreeGroup {
                    x: g.x,
                    y: g.y,
                    is_proxy: g.is_proxy == Some(true),
                    ascendancy_name: None,
                    is_ascendancy_start: false,
                },
            );
        }
        let mut ids: Vec<(u32, &RawNode)> = Vec::with_capacity(json.nodes.len());
        for (key, raw) in json.nodes.iter() {
            // :510 `self.nodes.root = nil`.
            if key == "root" {
                continue;
            }
            let id = pob_lua::tonumber(key)
                .and_then(as_id)
                .ok_or_else(|| TreeError(format!("PassiveTree: node key '{key}' is not an id")))?;
            ids.push((id, raw));
        }
        ids.sort_by_key(|(id, _)| *id);
        for (id, raw) in ids {
            let mut node = self.migrate_node(id, raw);
            self.classify_node(&mut node, raw);
            self.attach_group(&mut node);
            self.node_index.insert(id, self.nodes.len());
            self.process_node(&mut node)?;
            self.nodes.push(node);
        }
        Ok(())
    }

    /// :522-541 plus the `<= 3.09` passive-point stat line (:538-540).
    fn migrate_node(&self, id: u32, raw: &RawNode) -> TreeNode {
        let modern = self.version_num >= 3.1;
        let mut sd = if modern { raw.stats.clone() } else { raw.sd.clone() }.unwrap_or_default();
        let ppg = if modern {
            raw.granted_passive_points
        } else {
            raw.passive_points_granted
        }
        .unwrap_or(0.0);
        if self.version_num <= 3.09 && ppg > 0.0 {
            sd.push(format!(
                "Grants {} Passive Skill Point{}",
                tostring_num(ppg),
                if ppg > 1.0 { "s" } else { "" }
            ));
        }
        let pick = |m: Option<f64>, l: Option<f64>| if modern { m } else { l };
        TreeNode {
            id,
            dn: if modern { raw.name.clone() } else { raw.dn.clone() }.unwrap_or_default(),
            name: raw.name.clone(),
            sd,
            ty: NodeType::Normal,
            g: pick(raw.group, raw.g).and_then(|g| (g.fract() == 0.0).then_some(g as i64)),
            o: pick(raw.orbit, raw.o),
            oidx: pick(raw.orbit_index, raw.oidx),
            group: None,
            ascendancy_name: raw.ascendancy_name.clone(),
            // < 3.10: `node.spc[0]` (a 0-based Lua list, :526).
            class_start_index: if modern {
                raw.class_start_index
            } else {
                raw.spc.as_ref().and_then(|s| s.index(0).copied())
            },
            is_ascendancy_start: raw.is_ascendancy_start == Some(true),
            is_multiple_choice: raw.is_multiple_choice == Some(true),
            is_multiple_choice_option: raw.is_multiple_choice_option == Some(true),
            is_proxy: raw.is_proxy == Some(true),
            is_blighted: raw.is_blighted == Some(true),
            is_jewel_socket: raw.is_jewel_socket == Some(true),
            is_mastery: raw.is_mastery == Some(true),
            expansion_jewel: raw.expansion_jewel.clone(),
            passive_points_granted: ppg,
            granted_strength: pick(raw.granted_strength, raw.sa),
            granted_dexterity: pick(raw.granted_dexterity, raw.da),
            granted_intelligence: pick(raw.granted_intelligence, raw.ia),
            reminder_text: raw.reminder_text.clone(),
            mastery_effect_ids: raw.mastery_effects.iter().flatten().map(|e| e.effect).collect(),
            icon: raw.icon.clone().unwrap_or_default(),
            linked_id: Vec::new(),
            x: None,
            y: None,
            angle: None,
            stats: NodeStats::default(),
        }
    }

    /// :544-607 -- the type ladder in upstream order (first match wins).
    fn classify_node(&mut self, node: &mut TreeNode, raw: &RawNode) {
        if let Some(csi) = node.class_start_index {
            // `if node.classStartIndex` -- 0 (Scion) is true in Lua (L4).
            node.ty = NodeType::ClassStart;
            if let Some(cls) = as_index(Some(csi)).and_then(|i| self.classes.get_mut(i)) {
                cls.start_node_id = Some(node.id);
            }
            return;
        }
        if node.is_ascendancy_start {
            node.ty = NodeType::AscendClassStart;
            // Lua indexes `ascendNameMap[...]` unguarded; an alternate ascendancy (3.22+) is not in our map (P5).
            if let Some(e) = node.ascendancy_name.as_ref().and_then(|n| self.ascend_name_map.get(n)).copied() {
                self.classes[e.class_id].classes[e.ascend_class_id].start_node_id = Some(node.id);
            }
            return;
        }
        if raw.m == Some(true) || raw.is_mastery == Some(true) {
            node.ty = NodeType::Mastery;
            for effect in raw.mastery_effects.iter().flatten() {
                let Some(key) = as_id(effect.effect).map(u64::from) else {
                    unimplemented!("Classes/PassiveTree.lua:561 non-integral mastery effect id {}", effect.effect);
                };
                if !self.mastery_effects.contains_key(&key) {
                    let mut entry = MasteryEffect {
                        id: effect.effect,
                        sd: effect.stats.clone(),
                        stats: NodeStats::default(),
                    };
                    let holder = StatHolder {
                        id: entry.id,
                        ty: None,
                        dn: "",
                    };
                    process_stats(&holder, &mut entry.sd, &mut entry.stats, 1);
                    self.mastery_effects.insert(key, entry);
                }
                // else :568 `effect.stats = <the earlier split list>` -- the node's own copy of the stats is display only.
            }
            return;
        }
        if node.is_jewel_socket {
            node.ty = NodeType::Socket;
            self.sockets.insert(node.id, self.nodes.len());
            return;
        }
        let idx = self.nodes.len();
        if raw.ks == Some(true) || raw.is_keystone == Some(true) {
            node.ty = NodeType::Keystone;
            self.keystone_map.insert(node.dn.clone(), idx);
            self.keystone_map.insert(lower(&node.dn), idx);
            return;
        }
        if raw.not_ == Some(true) || raw.is_notable == Some(true) {
            node.ty = NodeType::Notable;
            match &node.ascendancy_name {
                None => {
                    // :583-588. "First seen, unless a later one has a group" -- with a hash order only well-defined because
                    // no name has two grouped nodes (3_13: 2 duplicate names, 1 grouped node each). Ascending id here.
                    let key = lower(&node.dn);
                    if !self.notable_map.contains_key(&key) || node.g.is_some() {
                        self.notable_map.insert(key, idx);
                    }
                }
                Some(asc) => {
                    self.ascendancy_map.insert(lower(&node.dn), idx);
                    let class_name = self.ascend_class_name(asc);
                    let list = self.class_notables.entry(class_name.clone()).or_default();
                    if class_name != "Scion" {
                        list.push(node.dn.clone());
                    }
                }
            }
            return;
        }
        node.ty = NodeType::Normal;
        // :597-606: Ascendant's own passives (and multiple-choice options) are listed as notables.
        let asc = node.ascendancy_name.as_deref();
        let dn = node.dn.as_str();
        let ascendant_passive = asc == Some("Ascendant")
            && !node.is_multiple_choice_option
            && !dn.contains("Dexterity")
            && !dn.contains("Intelligence")
            && !dn.contains("Strength")
            && !dn.contains("Passive");
        if (ascendant_passive || (node.is_multiple_choice_option && asc.is_some())) && asc != Some("Reliquarian") && asc != Some("Luminary")
        {
            let class_name = self.ascend_class_name(asc.unwrap_or(""));
            self.ascendancy_map.insert(lower(dn), idx);
            self.class_notables.entry(class_name).or_default().push(node.dn.clone());
        }
    }

    /// `self.ascendNameMap[name].class.name`; an unknown name (alternate ascendancies, P5) reads "" as in pob-ts.
    fn ascend_class_name(&self, asc: &str) -> String {
        self.ascend_name_map
            .get(asc)
            .map(|e| self.classes[e.class_id].name.clone())
            .unwrap_or_default()
    }

    /// :609-619 -- `self.groups[node.g]`; a Notable/Keystone without a group is a cluster-jewel node.
    fn attach_group(&mut self, node: &mut TreeNode) {
        match node.g.and_then(|g| self.groups.get_mut(&g).map(|grp| (g, grp))) {
            Some((g, grp)) => {
                node.group = Some(g);
                // Order-dependent upstream (last node wins); highest id here (D13). Display only.
                grp.ascendancy_name = node.ascendancy_name.clone();
                if node.is_ascendancy_start {
                    grp.is_ascendancy_start = true;
                }
            }
            None => {
                if matches!(node.ty, NodeType::Notable | NodeType::Keystone) {
                    self.cluster_node_map.insert(node.dn.clone(), self.nodes.len());
                }
            }
        }
    }

    /// :875-906 -- the true position, then the stat lines.
    fn process_node(&self, node: &mut TreeNode) -> Result<(), TreeError> {
        if let Some(g) = node.group {
            let grp = &self.groups[&g];
            let angle = as_index(node.o)
                .and_then(|o| self.orbit_angles_by_orbit.get(o))
                .and_then(|a| as_index(node.oidx).and_then(|i| a.get(i)))
                .copied()
                .ok_or_else(|| {
                    TreeError(format!(
                        "Classes/PassiveTree.lua:899: node {}: no orbit angle for o={:?} oidx={:?}",
                        node.id, node.o, node.oidx
                    ))
                })?;
            let radius = as_index(node.o)
                .and_then(|o| self.orbit_radii.get(o))
                .copied()
                .ok_or_else(|| TreeError(format!("Classes/PassiveTree.lua:900: node {}: no orbit radius", node.id)))?;
            node.angle = Some(angle);
            // MINEFIELD-RUST R2: `f64::sin/cos` = desktop UCRT.
            node.x = Some(grp.x + angle.sin() * radius);
            node.y = Some(grp.y - angle.cos() * radius);
        }
        let holder = StatHolder {
            id: f64::from(node.id),
            ty: Some(node.ty),
            dn: &node.dn,
        };
        let mut sd = std::mem::take(&mut node.sd);
        let mut stats = NodeStats::default();
        process_stats(&holder, &mut sd, &mut stats, 1);
        node.sd = sd;
        node.stats = stats;
        Ok(())
    }

    /// :620-650 -- `linkedId` is every `out`, then every `in`, each in Lua key order.
    fn build_links(&mut self) {
        for node in &mut self.nodes {
            let raw = &self.json.nodes[node.id.to_string().as_str()];
            for list in [&raw.out, &raw.in_].into_iter().flatten() {
                for (_, r) in list.entries() {
                    if let Some(other) = node_ref_id(r) {
                        node.linked_id.push(other);
                    }
                }
            }
        }
    }

    /// :713-721 -- every Normal node next to a class start gets a Condition flag naming that class.
    fn add_class_start_conditions(&mut self) {
        for c in 0..self.classes.len() {
            let Some(start) = self.classes[c].start_node_id.and_then(|id| self.node_idx(id)) else {
                continue;
            };
            let flag = format!("Condition:ConnectedTo{}Start", self.classes[c].name);
            for other in self.nodes[start].linked_id.clone() {
                let Some(i) = self.node_idx(other) else { continue };
                let node = &mut self.nodes[i];
                if node.ty == NodeType::Normal {
                    let src = format!("Tree:{other}");
                    node.stats.mod_list.mods.push(Rc::new(create_mod(
                        &flag,
                        Some(ModType::Flag),
                        Value::Bool(true),
                        Some(&src),
                        0,
                        0,
                        Vec::new(),
                    )));
                }
            }
        }
    }
}
