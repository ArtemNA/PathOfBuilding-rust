//! The `env` table `calcs.initEnv` builds (Modules/CalcSetup.lua:475-1975) and the two actor tables hanging off it --
//! pob-ts `src/calc/env.ts` (CALC-ARCH §7). Field names are upstream's in `snake_case`, so perform/defence (R4) read
//! line for line against CalcPerform/CalcDefence.
//!
//! The actor graph (R3-ARCH §2.2): `Env` owns `player`/`enemy` strongly; `CalcActor::enemy`/`player` are `Weak`;
//! `actor.mod_db` is a strong `StoreRef` whose `store.actor` is weak (`ModStore::set_actor`). The mod core's actor table
//! (`pob_mods::Actor`, what EvalMod reads through `store.actor`) keeps `actor.enemy`/`actor.player` as STRONG refs in
//! its `actors` map (pob-mods API) -- those are the only cycles, and `Drop for Env` cuts them.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_build::{BuildModel, ConfigState, InputValue};
use pob_items::{DataTable, Item};
use pob_mods::{ActorRef, StoreRef};
use pob_tree::PassiveSpec;

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
    /// CALC-ARCH §8: the stub `createActiveSkill` would otherwise build (CalcSetup.lua:1928-1940, "default Melee").
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
    pub spec: PassiveSpec,
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
    pub explode_sources: Vec<Rc<Item>>,
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

    /// `build.itemsTab.orderedSlots`.
    pub ordered_slots: Vec<Slot>,
    /// `build.itemsTab.items` -- every `<Item>` whose base resolved, by id, in XML order.
    pub items: IndexMap<u64, Rc<Item>>,

    /// Every upstream branch this port knowingly skips, as `"<File.lua>:<line> <what>"` (RUST-ARCH §5).
    pub unimplemented: Vec<String>,
}

impl Env {
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
        for a in [&self.player, &self.enemy] {
            if let Ok(a) = a.try_borrow() {
                a.actor.borrow_mut().actors.clear();
            }
        }
    }
}
