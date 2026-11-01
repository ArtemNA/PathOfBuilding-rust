//! The `env` table `calcs.initEnv` builds (Modules/CalcSetup.lua:475-1975) and the two actor tables hanging off it --
//! pob-ts `src/calc/env.ts` (CALC-ARCH §7). Field names are upstream's in `snake_case`, so perform/defence (R4) read
//! line for line against CalcPerform/CalcDefence.
//!
//! The actor graph (R3-ARCH §2.2): `Env` owns `player`/`enemy` strongly; `CalcActor::enemy`/`player` are `Weak`;
//! `actor.mod_db` is a strong `StoreRef` whose `store.actor` is weak (`ModStore::set_actor`). The mod core's actor table
//! (`pob_mods::Actor`, what EvalMod reads through `store.actor`) keeps `actor.enemy`/`actor.player` as STRONG refs in
//! its `actors` map (pob-mods API) -- those are the only cycles, and `Drop for Env` cuts them.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_build::{BuildModel, ConfigState, InputValue};
use pob_items::{DataTable, Item};
use pob_mods::{ActorRef, StoreRef};
use pob_tree::{PassiveSpec, SpecNode};

/// One key of `override.addNodes`: a node of `env.spec` (an index into `spec.nodes`), or a node that is not one -- a mastery-effect node
/// `PowerBuilder` makes (CalcsTab.lua:516-529) or a cluster notable of `tree.clusterNodeMap` (:714-736).
#[derive(Clone)]
pub enum OverrideNode {
    Spec(usize),
    Other(Rc<SpecNode>),
}

/// `override` of `calcs.initEnv(build, mode, override, specEnv)` (Calcs.lua:72-83 `CalcOverride`, CalcSetup.lua:485-1437): what the
/// calculator of `calcs.getMiscCalculator` (node power, item/gem comparisons) changes against the build. `None` = the field is nil.
#[derive(Clone, Default)]
pub struct CalcOverride {
    /// `override.addNodes` -- a set upstream (`{ [node] = true }`); here in the caller's order.
    pub add_nodes: Option<Vec<OverrideNode>>,
    /// `override.removeNodes` -- spec node indices (a set of node objects upstream: `removeNodes[node.id]` at :1393/:1408 is always nil).
    pub remove_nodes: Option<Vec<usize>>,
    /// `override.conditions` (:696-700): `modDB.conditions[flag] = true`.
    pub conditions: Option<Vec<String>>,
    /// `override.repSlotName` / `override.repItem` (:815-820): the item in that slot is replaced.
    pub rep_slot_name: Option<String>,
    pub rep_item: Option<Rc<Item>>,
    /// `override.toggleFlask` (:1368-1374): the flask is switched off if active, on if not.
    pub toggle_flask: Option<Rc<Item>>,
    /// `override.toggleTincture` (:1375-1381).
    pub toggle_tincture: Option<Rc<Item>>,
    /// `override.spec` (:501, :821): another passive spec instead of `build.spec`.
    pub spec: Option<Rc<PassiveSpec>>,
    /// `override.extraJewelFuncs` (:875, :905, :1425-1437) -- a ModList of `ExtraJewelFunc` mods.
    pub extra_jewel_funcs: Option<StoreRef>,
}

/// `env.player` / `env.enemy` (CALC-ARCH §7 `CalcActor`).
pub struct CalcActor {
    /// The mod core's view of this actor (`modDB.actor`): `mod_db`, `item_list`, `weapon_data1/2` and the
    /// `actor.enemy`/`actor.player` links EvalMod's `ActorCondition`/`getActor` follow.
    pub actor: ActorRef,
    pub mod_db: StoreRef,
    pub level: f64,
    /// `actor.output` -- filled by perform (R4).
    pub output: IndexMap<String, f64>,
    /// `actor.itemList[slotName]`, in insertion (= `orderedSlots`) order.
    pub item_list: IndexMap<String, Rc<Item>>,
    /// `item.weaponData[1]` or a copy of `data.unarmedWeaponData[classId]`.
    pub weapon_data1: DataTable,
    /// `item.weaponData[2]` or `{}`.
    pub weapon_data2: DataTable,
    pub enemy: Weak<RefCell<CalcActor>>,
    pub player: Weak<RefCell<CalcActor>>,
    /// CALC-ARCH §8: the stub `createActiveSkill` would otherwise build (CalcSetup.lua:1928-1940, "default Melee") -- the pre-R6 path
    /// only; with the skill phase on, perform reads `env.skills` (`crate::perform::MainSkill`) and this is `None`.
    pub main_skill: Option<MainSkillStub>,
    /// `actor.activeSkillList` -- the skill phase is P5, always empty here.
    pub active_skill_list: Vec<MainSkillStub>,
}

/// CALC-ARCH §8 stub of `env.player.mainSkill`.
pub struct MainSkillStub {
    pub skill_mod_list: StoreRef,
    pub granted_effect_name: &'static str,
}

/// `env.grantedSkills[i]` -- `copyTable(skill)` plus where it came from (:824-832 items / :1444-1452 nodes).
#[derive(Clone, Debug)]
pub struct GrantedSkill {
    pub skill_id: pob_mods::Value,
    pub level: pob_mods::Value,
    pub source: String,
    pub slot_name: Option<String>,
    pub source_item: Option<Rc<Item>>,
    /// `sourceNode` as a node id.
    pub source_node: Option<u32>,
}

/// The function of a `radiusJewelList` entry.
#[derive(Clone, Debug)]
pub enum RadiusFunc {
    /// `func.func` of an entry of `item.jewelData.funcList` -- a `pob_parser::jewel` function value.
    Visitor(pob_mods::Value),
    /// CalcSetup.lua:877-884, the default when the jewel has no `funcList`: tallies Str/Dex/Int of `out` into `data`.
    DefaultTally,
}

/// One `env.radiusJewelList` entry (CalcSetup.lua:887-895). `attributes` (`node.attributesInRadius[...]`) is not kept:
/// PassiveTree only ever makes it empty tables and nothing in the calc reads it.
#[derive(Clone, Debug)]
pub struct RadiusJewel {
    /// `node.nodesInRadius[item.jewelRadiusIndex] or {}` -- node ids, ascending.
    pub nodes: Vec<u32>,
    pub func: RadiusFunc,
    /// `func.type` ("Other", "Self", "SelfUnalloc", "Threshold", ...).
    pub ty: pob_mods::Value,
    pub item: Rc<Item>,
    pub node_id: u32,
    pub data: pob_mods::Table,
}

impl RadiusJewel {
    /// `rad.nodes[id]` (a hash lookup upstream).
    pub fn has(&self, id: u32) -> bool {
        self.nodes.binary_search(&id).is_ok()
    }
}

/// `env.requirementsTable[i]` (:1082-1090 items; the gem half is P5 and stays empty).
#[derive(Clone, Debug)]
pub struct ReqEntry {
    pub source: &'static str,
    pub source_item: Option<Rc<Item>>,
    pub source_slot: Option<String>,
    /// `sourceGem` of a gem entry (:1847-1853) -- (group, gem) indices into `socketGroupList` (0-based).
    pub source_gem: Option<(usize, usize)>,
    pub str_: Option<f64>,
    pub dex: Option<f64>,
    pub int: Option<f64>,
}

/// One `build.itemsTab.orderedSlots` entry (ItemsTab.lua:141-235).
#[derive(Clone, Debug)]
pub struct Slot {
    pub slot_name: String,
    pub label: String,
    /// `tonumber(slotName:match("%d+$") or slotName:match("%d+"))` (ItemSlotControl.lua:40).
    pub slot_num: Option<f64>,
    pub weapon_set: Option<f64>,
    pub node_id: Option<u32>,
    /// Index of the parent slot in `orderedSlots`.
    pub parent_slot: Option<usize>,
    /// The flask checkbox: `activeItemSet[slotName].active`.
    pub active: Option<bool>,
    /// `activeItemSet[slotName].selItemId`, or `spec.jewels[nodeId]` for a tree socket (ItemSlotControl.lua:88).
    pub sel_item_id: f64,
}

pub struct Env {
    pub build: Rc<BuildModel>,
    pub mode: &'static str,
    pub build_breakdown: bool,
    pub spec: Rc<PassiveSpec>,
    pub class_id: usize,
    /// The ConfigTab state behind `env.configInput` (`config.input`) / `env.configPlaceholder` (`config.placeholder`).
    pub config: ConfigState,
    pub calcs_input: IndexMap<String, InputValue>,

    pub mod_db: StoreRef,
    pub enemy_db: StoreRef,
    pub item_mod_db: StoreRef,
    pub enemy_level: f64,
    pub player: Rc<RefCell<CalcActor>>,
    pub enemy: Rc<RefCell<CalcActor>>,

    pub mode_buffs: bool,
    pub mode_combat: bool,
    pub mode_effective: bool,

    /// `env.allocNodes`: node id -> index into `spec.nodes`, insertion order (D13: pob-ts order, see init_env).
    pub alloc_nodes: IndexMap<u32, usize>,
    /// A ModList.
    pub initial_node_mod_db: StoreRef,
    pub keystones_added: IndexMap<String, bool>,
    /// `env.radiusJewelList` (CalcSetup.lua:871-935, R5.5b; `ParseMode::Full` only -- pob-ts has none).
    pub radius_jewel_list: Vec<RadiusJewel>,
    /// `env.extraRadiusNodeList`: node id -> index into `spec.nodes`. Upstream walks it with `pairs` (a hash order, D13); here
    /// the insertion order (jewel order, then ascending id).
    pub extra_radius_node_list: IndexMap<u32, usize>,
    pub granted_passives: IndexMap<u32, bool>,
    pub granted_skills: Vec<GrantedSkill>,
    pub granted_skills_nodes: Vec<GrantedSkill>,
    pub granted_skills_items: Vec<GrantedSkill>,
    /// What CalcSetup reads of `env.grantedSkills[i]` beyond [`GrantedSkill`] (`nameSpec`, `noSupports`, `triggered`, `triggerChance`),
    /// aligned with `granted_skills` / `granted_skills_items`.
    pub granted_skills_extra: Vec<crate::skills::GrantedSkillExtra>,
    pub granted_skills_items_extra: Vec<crate::skills::GrantedSkillExtra>,
    /// `env.explodeSources`: the nodes' (`buildModListForNodeList`), then the items' (:1423 `tableConcat`).
    pub explode_sources: Vec<crate::skills_tab::ExplodeSource>,
    pub item_warnings: IndexMap<String, Vec<String>>,
    pub flasks: Vec<Rc<Item>>,
    pub tinctures: Vec<Rc<Item>>,
    /// `env.flaskSlotMap[item] = n`, keyed by the slot name the flask sits in.
    pub flask_slot_map: IndexMap<String, f64>,
    pub flask_slot_occupied: IndexMap<String, bool>,
    pub requirements_table_items: Vec<ReqEntry>,
    pub requirements_table_gems: Vec<ReqEntry>,
    pub requirements_table: Vec<ReqEntry>,
    pub main_socket_group: f64,
    pub aegis_mod_list: Option<StoreRef>,
    pub the_iron_mass: Option<StoreRef>,
    pub weapon_mod_list1: Option<StoreRef>,

    /// `node.grantedSkills` (upstream writes it onto the shared node table; kept here).
    pub node_granted_skills: IndexMap<u32, Vec<GrantedSkill>>,
    /// `node.finalModList`.
    pub node_final_mod_list: IndexMap<u32, StoreRef>,
    /// `jewelData.limitDisabled = true` (upstream writes it onto the shared item table; kept here, by item id).
    pub jewel_limit_disabled: Vec<f64>,
    /// The class-name VALUES of `env.itemModDB.conditions[<unique title>]` (CalcSetup.lua:1272-1274; `ModStore.conditions` holds
    /// booleans, so the store keeps `true`). Read by the Forbidden Flame/Flesh check (:1408), which an accelerated re-init repeats
    /// on the kept `itemModDB` (R7.2b).
    pub item_class_restrictions: IndexMap<String, String>,

    /// `build.itemsTab.orderedSlots`.
    pub ordered_slots: Vec<Slot>,
    /// `build.itemsTab.items` -- every `<Item>` whose base resolved, by id, in XML order.
    pub items: IndexMap<u64, Rc<Item>>,

    /// The skill phase (R6.3, `CalculateOptions.skills`): `build.skillsTab`, `env.player.activeSkillList`, `mainSkill`. `None` = the
    /// pre-R6 path (no skills; perform's stub main skill only).
    pub skills: Option<crate::skills::SkillsState>,
    /// `activeSkill.skillModList` after CalcPerform.lua:1296 (perform gives every skill a fresh child ModList of its `baseSkillModList`),
    /// aligned with `skills.active_skill_list`; [`crate::active_skill::ActiveSkill::skill_mod_list`] stays the `baseSkillModList`.
    /// Empty before perform and on the pre-R6 path.
    pub skill_mod_lists: Vec<StoreRef>,

    /// Every upstream branch this port knowingly skips, as `"<File.lua>:<line> <what>"` (RUST-ARCH §5).
    pub unimplemented: Vec<String>,

    /// `env.limitedSkills` (R7.2): the uuids `calcs.buildActiveSkill` (Calcs.lua:361-371) marks so that a nested run does not
    /// recurse into them again. nil (`None`) on an env `buildActiveSkill` did not make; a set -- upstream walks it with `pairs`
    /// only to copy it (R13: the order cannot matter).
    pub limited_skills: Option<BTreeSet<String>>,
    /// The [`crate::session::CalcSession`] that built this env (what upstream reaches as the globals `calcs`/`GlobalCache`):
    /// nested runs (`getCachedOutputValue`, triggers, mirages) go through it. Weak -- the session's cache owns envs.
    pub session: Weak<crate::session::SessionCore>,
    /// `env.minion = env.player.mainSkill.minion` (CalcPerform.lua:1316, R7.10): the main skill's minion actor while perform runs (the same
    /// `Rc` as that skill's [`crate::minion::Minion::actor`]); `None` = nil.
    pub minion: Option<Rc<RefCell<CalcActor>>>,
    /// `env.override` (CalcSetup.lua:502): what this env was built with (`None` = `{}`); the accelerated re-init of `calcFullDPS` reads
    /// its `conditions` again (:696).
    pub calc_override: Option<Rc<CalcOverride>>,
}

impl Env {
    /// `env.player.activeSkillList[i + 1].skillModList` after CalcPerform.lua:1296 (perform's child list; before perform the base list), or
    /// the detached main skill's own list for [`crate::skills::DETACHED`] (R7.9).
    pub fn skill_mod_list(&self, i: usize) -> StoreRef {
        crate::perform::skill_list_of(self, i)
    }

    /// `build.itemsTab.items[id]`.
    pub fn item(&self, id: f64) -> Option<&Rc<Item>> {
        if id >= 0.0 && id.fract() == 0.0 {
            self.items.get(&(id as u64))
        } else {
            None
        }
    }
}

impl Drop for Env {
    /// Cuts the strong `actor.enemy`/`actor.player` links of the mod core's actor tables (the only cycles of the graph,
    /// `player.player = player` among them), so dropping the `Env` frees every actor and store (R3-ARCH §2.2).
    fn drop(&mut self) {
        if let Some(m) = &self.minion
            && let Ok(a) = m.try_borrow()
        {
            a.actor.borrow_mut().actors.clear();
        }
        for a in [&self.player, &self.enemy] {
            if let Ok(a) = a.try_borrow() {
                a.actor.borrow_mut().actors.clear();
            }
        }
    }
}
