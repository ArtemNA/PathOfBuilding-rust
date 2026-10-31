//! `calcs.initEnv(build, "MAIN")` -- Modules/CalcSetup.lua:475-1975 without `override`/`specEnv` (no accelerators, no
//! minions, no CALCS mode) and without the skill half (:1461-1946 bar the three pieces CALC-ARCH §7 (18) names).
//! Executable spec: pob-ts `src/calc/initEnv.ts`; truth: LuaJIT (fixtures `tests/fixtures/<build>.luajit.json`, key `env`).
//!
//! The phase order IS the mod insertion order, and the insertion order is the summation order (MINEFIELD L10): nothing
//! here may be reordered "for clarity". The phase numbers in the comments are CALC-ARCH §7 (1)-(19); line numbers are
//! this checkout's (`ed354c2f8`). What is deliberately not done is pushed to `env.unimplemented` as `file:line`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_build::{BuildModel, ConfigModLists, ConfigState, InputValue, active_item_set, active_skill_groups, active_spec};
use pob_data::constants::{constants, monster_table_at};
use pob_data::pantheons::pantheons;
use pob_data::rules::rules;
use pob_items::{DataTable, Item, ItemOptions, NumKey};
use pob_lua::number::{max2, min2};
use pob_mods::{Actor, Mod, ModStore, ModType, StoreRef, Value};
use pob_tree::{ClusterJewel, ConqueredBy, PassiveSpec, SpecJewel, TreeError};

use crate::calc_lib;
use crate::env::{CalcActor, Env, GrantedSkill, MainSkillStub, RadiusFunc, RadiusJewel, ReqEntry, Slot};
use crate::setup::{
    apply_soul_mod, base, build_mod_list_for_node_list, cc, cfg_source, cond, cond_neg, init_mod_db, keyword_flag, list_mods, lua_gsub,
    merge_db, merge_keystones, misc, mod_flag, mult, n, new_mod, ok, s, tag, temp_list, title_case_rarity,
};
use crate::skills::{GrantedSkillExtra, SkillsState};
use crate::skills_tab::{ByRef, ExplodeSource};

/// What a caller may hand `init_env` instead of letting it build them (the oracle tests parse items themselves to record
/// ModCache misses per item).
#[derive(Default)]
pub struct InitEnvOptions {
    pub spec: Option<PassiveSpec>,
    pub items: Option<IndexMap<u64, Rc<Item>>>,
    /// How every mod line of the build is read (R5.4): `Full` (desktop, default) or `CacheOnly` (pob-ts).
    pub parse_mode: pob_parser::ParseMode,
}

#[derive(Debug)]
pub struct InitEnvError(pub String);

impl std::fmt::Display for InitEnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InitEnvError {}

impl From<TreeError> for InitEnvError {
    fn from(e: TreeError) -> Self {
        InitEnvError(e.0)
    }
}

/// R7-perf: wall time of initEnv by phase, summed over every initEnv of the thread while on (off by default: one `Cell` read per
/// phase mark). Diagnostics only -- nothing in the calculation reads it.
pub mod profile {
    use std::cell::{Cell, RefCell};
    use std::time::Instant;

    use indexmap::IndexMap;

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static ACC: RefCell<IndexMap<&'static str, (f64, usize)>> = RefCell::new(IndexMap::new());
    }

    /// Turns the per-phase timing on or off for this thread.
    pub fn enable(on: bool) {
        ON.with(|c| c.set(on));
    }

    /// The phases timed since the last `take`, in first-seen order: `(phase, total ms, count)`.
    pub fn take() -> Vec<(&'static str, f64, usize)> {
        ACC.with(|a| {
            std::mem::take(&mut *a.borrow_mut())
                .into_iter()
                .map(|(k, (ms, n))| (k, ms, n))
                .collect()
        })
    }

    /// Adds `ms` to `phase` (a no-op while off).
    pub fn add(phase: &'static str, ms: f64) {
        if ON.with(Cell::get) {
            ACC.with(|a| {
                let mut a = a.borrow_mut();
                let e = a.entry(phase).or_insert((0.0, 0));
                e.0 += ms;
                e.1 += 1;
            });
        }
    }

    /// Times the stretch since the previous mark.
    pub(crate) struct Marker(Option<Instant>);

    impl Marker {
        pub(crate) fn new() -> Marker {
            Marker(ON.with(Cell::get).then(Instant::now))
        }

        pub(crate) fn mark(&mut self, phase: &'static str) {
            if let Some(t) = self.0 {
                let now = Instant::now();
                add(phase, (now - t).as_secs_f64() * 1000.0);
                self.0 = Some(now);
            }
        }
    }
}

// --- ItemsTab.lua:37-235 `orderedSlots` -----------------------------------------------------------------------------

/// ItemsTab.lua:37
const BASE_SLOTS: [&str; 18] = [
    "Weapon 1",
    "Weapon 2",
    "Helmet",
    "Body Armour",
    "Gloves",
    "Boots",
    "Amulet",
    "Ring 1",
    "Ring 2",
    "Ring 3",
    "Belt",
    "Graft 1",
    "Graft 2",
    "Flask 1",
    "Flask 2",
    "Flask 3",
    "Flask 4",
    "Flask 5",
];

/// ItemsTab.lua:188 -- the seven slots that get their own six abyssal sockets.
const ABYSSAL_PARENTS: [&str; 7] = ["Weapon 1", "Weapon 2", "Helmet", "Gloves", "Body Armour", "Boots", "Belt"];

/// ItemSlotControl.lua:40 `tonumber(slotName:match("%d+$") or slotName:match("%d+"))`.
fn slot_num_of(slot_name: &str) -> Option<f64> {
    let b = slot_name.as_bytes();
    let digits_at = |start: usize| -> f64 {
        let end = b[start..].iter().position(|c| !c.is_ascii_digit()).map_or(b.len(), |p| start + p);
        pob_lua::tonumber(&slot_name[start..end]).expect("digits")
    };
    // `%d+$`: the maximal digit run touching the end.
    let tail = b.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    if tail > 0 {
        return Some(digits_at(b.len() - tail));
    }
    b.iter().position(u8::is_ascii_digit).map(digits_at)
}

/// `ItemsTab:ItemsTab` (:141-235) builds `orderedSlots` in this exact order; initEnv's item phase walks it three times, so
/// the order is part of the mod insertion order (L10). The jewel-socket half comes from `build.latestTree` upstream
/// (D19.4); this port uses the build's OWN tree (`spec.tree.sockets`, ascending id as `table.sort(socketOrder)` gives) --
/// on the 3.13 goldens the two sets are identical (57 ids, checked against the LuaJIT `orderedSlots`), as in pob-ts.
fn build_ordered_slots(spec: &PassiveSpec, set: &pob_build::ItemSetModel) -> Vec<Slot> {
    let mut ordered: Vec<Slot> = Vec::new();
    let gear = |slot_name: String, label: &str| -> Slot {
        let entry = set.slots.get(&slot_name);
        Slot {
            slot_num: slot_num_of(&slot_name),
            label: label.to_owned(),
            sel_item_id: entry.map_or(0.0, |e| e.item_id),
            active: entry.map(|e| e.active),
            weapon_set: None,
            node_id: None,
            parent_slot: None,
            slot_name,
        }
    };
    for slot_name in BASE_SLOTS {
        let slot_idx = ordered.len();
        ordered.push(gear(slot_name.to_owned(), slot_name));
        if slot_name.contains("Weapon") {
            ordered[slot_idx].weapon_set = Some(1.0);
            let swap_idx = ordered.len();
            let mut swap = gear(format!("{slot_name} Swap"), slot_name);
            swap.weapon_set = Some(2.0);
            ordered.push(swap);
            for i in 1..=6 {
                let mut abyssal = gear(format!("{slot_name} Swap Abyssal Socket {i}"), &format!("Abyssal #{i}"));
                abyssal.parent_slot = Some(swap_idx);
                abyssal.weapon_set = Some(2.0);
                ordered.push(abyssal);
            }
        }
        if ABYSSAL_PARENTS.contains(&slot_name) {
            for i in 1..=6 {
                let mut abyssal = gear(format!("{slot_name} Abyssal Socket {i}"), &format!("Abyssal #{i}"));
                abyssal.parent_slot = Some(slot_idx);
                if slot_name.contains("Weapon") {
                    abyssal.weapon_set = Some(1.0);
                }
                ordered.push(abyssal);
            }
        }
    }
    let mut sockets: Vec<u32> = spec.tree.sockets.keys().copied().collect();
    sockets.sort_unstable();
    for node_id in sockets {
        let slot_name = format!("Jewel {node_id}");
        ordered.push(Slot {
            slot_num: slot_num_of(&slot_name),
            label: "Socket".into(),
            // ItemSlotControl.lua:88 -- a socket's selItemId IS spec.jewels[nodeId].
            sel_item_id: spec.jewels.get(f64::from(node_id)).copied().unwrap_or(0.0),
            active: None,
            weapon_set: None,
            node_id: Some(node_id),
            parent_slot: None,
            slot_name,
        });
    }
    ordered
}

/// `build.itemsTab.items` -- ItemsTab.lua:1216-1266; only items whose base resolved are kept (:1264).
pub fn parse_items(build: &BuildModel, mode: pob_parser::ParseMode) -> IndexMap<u64, Rc<Item>> {
    let mut items = IndexMap::new();
    for im in &build.items {
        let item = Item::parse(&im.raw, &item_options(im, mode));
        if item.base.is_none() {
            continue;
        }
        items.insert(im.id as u64, Rc::new(item));
    }
    items
}

/// What `BuildSubgraph` reads off `build.itemsTab.items[itemId]` -- `GetJewel` (PassiveSpec.lua:901-910) returns the item
/// only when it has `jewelData`; the `clusterJewel*` fields are Item.lua's (`BuildModList`, :2495-2520).
pub fn cluster_jewel_of(item: &Item) -> Option<ClusterJewel> {
    let jd = item.jewel_data.as_ref()?;
    let text = |k: &str| -> Option<String> {
        let v = jd.get(k);
        match v.as_str() {
            Some(s) => Some(s.to_owned()),
            None if !v.truthy() => None,
            None => panic!(
                "Classes/PassiveSpec.lua:1970: jewelData.{k} is a {} (expected a string)",
                v.type_name()
            ),
        }
    };
    let list = |k: &str| -> Vec<String> {
        let v = jd.get(k);
        if !v.is_table() {
            return Vec::new();
        }
        v.ipairs()
            .into_iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| panic!("Classes/PassiveSpec.lua:1970: jewelData.{k} holds a {}", x.type_name()))
            })
            .collect()
    };
    Some(ClusterJewel {
        cluster_jewel: item.cluster_jewel,
        valid: jd.get("clusterJewelValid").truthy(),
        keystone: text("clusterJewelKeystone"),
        notables: list("clusterJewelNotables"),
        skill: text("clusterJewelSkill"),
        socket_count_override: jd.num("clusterJewelSocketCountOverride"),
        socket_count: jd.num("clusterJewelSocketCount"),
        node_count: jd.num("clusterJewelNodeCount"),
        nothingness_count: jd.num("clusterJewelNothingnessCount"),
        added_mods: list("clusterJewelAddedMods"),
        inc_effect: jd.num("clusterJewelIncEffect"),
    })
}

/// What BuildAllDependsAndPaths reads off a socketed item (PassiveSpec.lua:1110-1203): `item.id`, `jewelRadiusIndex`,
/// `jewelData` and its `limitDisabled`/`conqueredBy`.
pub fn spec_jewel_of(item: &Item) -> Result<SpecJewel, TreeError> {
    let jd = item.jewel_data.as_ref();
    Ok(SpecJewel {
        id: item.id,
        jewel_radius_index: item.jewel_radius_index,
        has_jewel_data: jd.is_some(),
        limit_disabled: jd.is_some_and(|d| d.get("limitDisabled").truthy()),
        conquered_by: match jd {
            Some(d) => ConqueredBy::from_value(&d.get("conqueredBy"))?.map(Rc::new),
            None => None,
        },
    })
}

/// `spec:PostLoad()` (PassiveSpec.lua:251, run by Build.lua:680 once the items tab has loaded): the cluster-jewel subgraphs
/// of the spec, built from the build's items.
pub fn post_load_spec(spec: &mut PassiveSpec, items: &IndexMap<u64, Rc<Item>>) -> Result<(), TreeError> {
    // The items tab is loaded before the tree (Build.lua:655-677), so BuildAllDependsAndPaths sees the jewels' conquests.
    let mut jewels = Vec::new();
    for &(_, item_id) in &spec.jewels.0 {
        if item_id < 0.0 || item_id.fract() != 0.0 || jewels.iter().any(|(id, _)| *id == item_id) {
            continue;
        }
        if let Some(item) = items.get(&(item_id as u64)) {
            jewels.push((item_id, Rc::new(spec_jewel_of(item)?)));
        }
    }
    spec.set_jewel_items(jewels);
    let lookup = |item_id: f64| -> Option<Rc<ClusterJewel>> {
        if item_id < 0.0 || item_id.fract() != 0.0 {
            return None;
        }
        items.get(&(item_id as u64)).and_then(|i| cluster_jewel_of(i)).map(Rc::new)
    };
    spec.post_load(&lookup)
}

/// The `<Item>` attributes as `Item::parse` wants them.
pub fn item_options(im: &pob_build::ItemModel, mode: pob_parser::ParseMode) -> ItemOptions {
    ItemOptions {
        parse_mode: mode,
        id: Some(im.id),
        variant: im.variant,
        variant_alt: [im.variant_alt, im.variant_alt2, im.variant_alt3, im.variant_alt4, im.variant_alt5],
        mod_ranges: im.mod_ranges.iter().map(|r| (r.id, r.range)).collect(),
    }
}

// --- the item-phase helpers -------------------------------------------------------------------------------------------

/// `tonumber(slotName:match("Flask (%d+)"))`: the first "Flask " followed by a digit run.
fn flask_num(slot_name: &str) -> Option<f64> {
    let b = slot_name.as_bytes();
    let mut from = 0;
    while let Some(p) = slot_name[from..].find("Flask ") {
        let start = from + p + "Flask ".len();
        let len = b[start..].iter().take_while(|c| c.is_ascii_digit()).count();
        if len > 0 {
            return pob_lua::tonumber(&slot_name[start..start + len]);
        }
        from = from + p + 1;
    }
    None
}

/// `s:gsub(" ", "")`.
fn no_spaces(x: &str) -> String {
    x.replace(' ', "")
}

/// `s:gsub("%s+", "")` -- `%s` is ASCII space, \t \n \v \f \r (R3).
fn no_whitespace(x: &str) -> String {
    x.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
        .collect()
}

/// The seven influence multipliers of :1206 / :1290 and the item field each reads.
const INFLUENCE_MULTS: [(&str, &str); 7] = [
    ("CorruptedItem", "corrupted"),
    ("ShaperItem", "shaper"),
    ("ElderItem", "elder"),
    ("WarlordItem", "adjudicator"),
    ("HunterItem", "basilisk"),
    ("CrusaderItem", "crusader"),
    ("RedeemerItem", "eyrie"),
];

/// `item[property]`.
fn item_has(item: &Item, property: &str) -> bool {
    if property == "corrupted" {
        item.corrupted
    } else {
        item.influence(property)
    }
}

/// `db.multipliers[key] = (db.multipliers[key] or 0) + by`.
fn bump_mult(db: &StoreRef, key: &str, by: f64) {
    let mut d = db.borrow_mut();
    let cur = d.multipliers.get(key).copied().unwrap_or(0.0);
    d.multipliers.insert(Rc::from(key), cur + by);
}

fn set_mult(db: &StoreRef, key: &str, v: f64) {
    db.borrow_mut().multipliers.insert(Rc::from(key), v);
}

fn set_cond(db: &StoreRef, key: &str, v: bool) {
    db.borrow_mut().conditions.insert(Rc::from(key), v);
}

/// `item.modList or item.slotModList[slotNum]` as shared mod objects. `nil` (no list at all) is the caller's business.
fn src_list_of(item: &Item, slot_num: Option<f64>) -> Option<Vec<Rc<Mod>>> {
    if let Some(list) = &item.mod_list {
        return Some(list.iter().map(|m| Rc::new(m.m.clone())).collect());
    }
    let slots = item.slot_mod_list.as_ref()?;
    let k = slot_num?;
    if k >= 1.0 && k.fract() == 0.0 {
        slots.get(k as usize - 1).map(|l| l.iter().map(|m| Rc::new(m.m.clone())).collect())
    } else {
        None
    }
}

fn has_socketed_in(m: &Mod) -> bool {
    m.tags.iter().any(|t| t.ty().is_str("SocketedIn"))
}

/// Config `input[key]` with Lua truthiness (L4).
fn input_truthy(config: &ConfigState, key: &str) -> bool {
    !matches!(config.input.get(key), None | Some(InputValue::Bool(false)))
}

/// A `DataTable` (`weaponData[n]`, `unarmedWeaponData[classId]`) as the mod core's table.
fn data_table_to_table(d: &DataTable) -> pob_mods::Table {
    let mut t = pob_mods::Table::default();
    for (k, v) in &d.0 {
        t.hash.insert(Rc::from(k.as_str()), v.clone());
    }
    t
}

/// The Gloves/Boots branches (:1244-1270): merge the source list, scale a copy of it, then merge the scaled copy back in
/// with `skipNonAdditive` so only BASE/INC/MORE double up.
fn merge_scaled(src_list: &[Rc<Mod>], scale: f64) -> Vec<Rc<Mod>> {
    let combined = ModStore::new_list(None);
    for m in src_list {
        ok(combined.borrow_mut().merge_mod(m.clone(), false));
    }
    let scaled = ModStore::new_list(None);
    ok(scaled.borrow_mut().scale_add_list(&list_mods(&combined), scale, false));
    for m in list_mods(&scaled) {
        ok(combined.borrow_mut().merge_mod(m, true));
    }
    list_mods(&combined)
}

/// CalcSetup.lua:875-904: one `radiusJewelList` entry per function of the jewel (`item.jewelData.funcList`, or the default
/// tally), and the unallocated nodes in radius of every non-`Self` function into `extraRadiusNodeList`.
fn add_radius_jewel(env: &mut Env, item: &Rc<Item>, socket_id: u32, radius_index: f64) {
    // `(item.jewelData and item.jewelData.funcList) or { default }` -- an empty funcList table is truthy (L4).
    let funcs: Vec<(RadiusFunc, Value)> = match item.jewel_data.as_ref().map(|jd| jd.get("funcList")) {
        Some(Value::Table(t)) => t
            .array
            .iter()
            .take_while(|f| !f.is_nil())
            .map(|f| (RadiusFunc::Visitor(f.get("func")), f.get("type")))
            .collect(),
        Some(Value::Nil | Value::Bool(false)) | None => vec![(RadiusFunc::DefaultTally, Value::str("Self"))],
        Some(other) => panic!(
            "Modules/CalcSetup.lua:886: bad argument #1 to 'ipairs' (table expected, got {})",
            other.type_name()
        ),
    };
    for (func, ty) in funcs {
        // `local node = env.spec.nodes[slot.nodeId]` -- the socket is allocated (:845), so it exists.
        let socket = env
            .spec
            .node(socket_id)
            .unwrap_or_else(|| panic!("Modules/CalcSetup.lua:888: attempt to index local 'node' (a nil value)"));
        let nir = socket.nodes_in_radius();
        // `nodesInRadius[item.jewelRadiusIndex]` -- an index the tree's table does not have is nil.
        let list = nir.and_then(|l| {
            (radius_index >= 1.0 && radius_index.fract() == 0.0)
                .then(|| l.get(radius_index as usize - 1))
                .flatten()
        });
        let nodes = list.cloned().unwrap_or_default();
        if !ty.is_str("Self") && nir.is_some() {
            let Some(list) = list else {
                panic!("Modules/CalcSetup.lua:898: bad argument #1 to 'pairs' (table expected, got nil)");
            };
            for &id in list {
                // `env.extraRadiusNodeList[nodeId] = env.spec.nodes[nodeId]` -- a nil value leaves no key.
                if !env.alloc_nodes.contains_key(&id)
                    && let Some(idx) = env.spec.node_idx(id)
                {
                    env.extra_radius_node_list.insert(id, idx);
                }
            }
        }
        env.radius_jewel_list.push(RadiusJewel {
            nodes,
            func,
            ty,
            item: item.clone(),
            node_id: socket_id,
            data: pob_mods::Table::default(),
        });
    }
}

fn new_actor(mod_db: &StoreRef, level: f64) -> Rc<RefCell<CalcActor>> {
    let actor = Rc::new(RefCell::new(Actor {
        mod_db: Some(mod_db.clone()),
        output: Some(IndexMap::new()),
        active_skill_list: Some(Vec::new()),
        item_list: Some(IndexMap::new()),
        ..Actor::default()
    }));
    Rc::new(RefCell::new(CalcActor {
        actor,
        mod_db: mod_db.clone(),
        level,
        output: IndexMap::new(),
        item_list: IndexMap::new(),
        weapon_data1: DataTable::default(),
        weapon_data2: DataTable::default(),
        enemy: Weak::new(),
        player: Weak::new(),
        main_skill: None,
        active_skill_list: Vec::new(),
    }))
}

// --- the entry point ----------------------------------------------------------------------------------------------

/// What the body of initEnv reads of [`InitEnvOptions`] once the spec and the items are in hand.
struct InitEnvMode {
    parse_mode: pob_parser::ParseMode,
}

/// `calcs.initEnv(build, "MAIN")` WITHOUT the skill phase -- the pre-R6 path its callers (the "= pob-ts" gates, the R3/R5 env gates,
/// `examples/bench_env.rs`) pin. The skill phase: [`init_env_with`] / `CalculateOptions::skills` (default on).
pub fn init_env(build: Rc<BuildModel>, opts: InitEnvOptions) -> Result<Env, InitEnvError> {
    init_env_with(build, opts, false)
}

/// `calcs.initEnv(build, "MAIN")` -- CalcSetup.lua:475-1975, phases (1)-(19) of CALC-ARCH §7; `skills` = the skill phase (R6.3,
/// R6-ARCH §2 p. 5: socketedGems, the granted-skill merge, supports, active skills, the main skill, `requirementsTableGems`).
pub fn init_env_with(build: Rc<BuildModel>, opts: InitEnvOptions, skills: bool) -> Result<Env, InitEnvError> {
    init_env_seeded(build, opts, skills, None)
}

/// [`init_env_with`] for a [`crate::session::CalcSession`] run (R7.2): `carry` = the writes earlier initEnvs of the process left on
/// the shared `data.skills` tables (`grantedEffect.fromItem = true` CalcSetup.lua:1633-1634, `levels[level].cost = {}`
/// SkillsTab.lua:ProcessSocketGroup) -- upstream's next initEnv starts from them, so the skill phase here does too.
pub fn init_env_seeded(
    build: Rc<BuildModel>,
    opts: InitEnvOptions,
    skills: bool,
    carry: Option<&crate::session::SkillsCarry>,
) -> Result<Env, InitEnvError> {
    let mut prof = profile::Marker::new();
    let parse_mode = opts.parse_mode;
    let (spec, parsed_items) = load_spec(&build, opts)?;
    prof.mark("spec: load + PostLoad");
    init_env_core(build, spec, parsed_items, parse_mode, None, skills, carry)
}

/// The build state upstream keeps on `build` and every initEnv reads unchanged (R7-perf): `build.spec` after `PostLoad`,
/// `build.itemsTab.items`, `build.configTab` with the lists its load-time `BuildModList` made, and `build.skillsTab` as
/// `SkillsTab:Load` + `UpdateSocketGroups` left it (Build.lua:691) -- BEFORE the granted-skill merge of an initEnv
/// (CalcSetup.lua:1448-1563), which every initEnv makes on its own copy of the tab.
///
/// Why sharing is exact: after load nothing in initEnv/perform takes the spec mutably (`Env::spec` is an `Rc`), and
/// `SkillsState::load` is a function of the model, the ordered slots (spec + item set) and the items -- all fixed for a build.
/// The mod objects of the spec and of the config lists are shared between runs, as upstream's (`env.spec = build.spec`,
/// CalcSetup.lua:486; `modDB:AddList(build.configTab.modList)`, :681-684): a `tag.div` written by one run is seen by the next
/// (MINEFIELD-RUST R4), as on LuaJIT. Each env gets its own copy of the config state (`EstimatePlayerProgress` rebuilds it).
pub struct BuildShared {
    pub spec: Rc<PassiveSpec>,
    pub items: ParsedItems,
    /// `SkillsTab:Load` + `UpdateSocketGroups` on the ordered slots of `spec`, or the Lua error it raised (reported at the
    /// point of initEnv where the load used to run).
    pub skills_tab: Result<crate::skills_tab::SkillsTab, crate::calc_tools::LuaError>,
    /// `build.configTab` after its `BuildModList` (which runs `UpdateLevel`), and the lists it made.
    pub config: ConfigState,
    pub config_lists: ConfigModLists,
}

impl BuildShared {
    /// Loads the spec (+ `PostLoad` in `ParseMode::Full`) and the skills tab of `build` over `items`.
    pub fn new(build: &BuildModel, items: ParsedItems, parse_mode: pob_parser::ParseMode) -> Result<BuildShared, InitEnvError> {
        let opts = InitEnvOptions {
            spec: None,
            items: Some(items),
            parse_mode,
        };
        let mut prof = profile::Marker::new();
        let (spec, items) = load_spec(build, opts)?;
        prof.mark("once per session: spec load + PostLoad");
        let (config, config_lists) = load_config(build, parse_mode)?;
        prof.mark("once per session: config BuildModList");
        let slots = build_ordered_slots(&spec, active_item_set(build));
        let skills_tab = SkillsState::load_tab(build, &slots, &items);
        prof.mark("once per session: SkillsTab:Load");
        Ok(BuildShared {
            spec,
            items,
            skills_tab,
            config,
            config_lists,
        })
    }
}

/// [`init_env_seeded`] over a [`BuildShared`] (the session path, R7-perf): no spec load, no `SkillsTab:Load` -- a copy of the
/// shared tab instead.
pub fn init_env_shared(
    build: Rc<BuildModel>,
    shared: &BuildShared,
    parse_mode: pob_parser::ParseMode,
    carry: Option<&crate::session::SkillsCarry>,
) -> Result<Env, InitEnvError> {
    init_env_core(
        build,
        shared.spec.clone(),
        shared.items.clone(),
        parse_mode,
        Some(shared),
        true,
        carry,
    )
}

/// `build.itemsTab.items`: item id -> the parsed item.
type ParsedItems = IndexMap<u64, Rc<Item>>;

/// `build.configTab` as the load leaves it: the XML's config set, then `BuildModList` -- the lists `build.configTab.modList`/
/// `.enemyModList` initEnv adds (CalcSetup.lua:681-684); it also runs `UpdateLevel`, which sets `configTab.enemyLevel`.
fn load_config(build: &BuildModel, parse_mode: pob_parser::ParseMode) -> Result<(ConfigState, ConfigModLists), InitEnvError> {
    let mut config = ConfigState::from_build(build);
    config.parse_mode = parse_mode;
    let lists = config.build_mod_list(build.level).map_err(|e| InitEnvError(e.to_string()))?;
    Ok((config, lists))
}

/// The spec (`PassiveSpec:Load`, then `PostLoad` in `ParseMode::Full`) and `build.itemsTab.items`, from `opts` or loaded.
fn load_spec(build: &BuildModel, opts: InitEnvOptions) -> Result<(Rc<PassiveSpec>, ParsedItems), InitEnvError> {
    let mut spec = match opts.spec {
        Some(s) => s,
        None => PassiveSpec::load_mode(active_spec(build), opts.parse_mode)?,
    };
    // `build.itemsTab.items` (ItemsTab:Load) exists before any calculation; the spec's PostLoad needs it.
    let parsed_items = match opts.items {
        Some(i) => i,
        None => parse_items(build, opts.parse_mode),
    };
    // Build.lua:680 `PostLoad` -> BuildClusterJewelGraphs (R5.5a). `ParseMode::CacheOnly` reproduces pob-ts, which has no
    // cluster graphs (its gates -- env (a), perform/defence/full fixtures -- are pinned without them).
    if opts.parse_mode == pob_parser::ParseMode::Full && !spec.post_loaded {
        post_load_spec(&mut spec, &parsed_items)?;
    }
    Ok((Rc::new(spec), parsed_items))
}

#[allow(clippy::too_many_lines)]
fn init_env_core(
    build: Rc<BuildModel>,
    spec: Rc<PassiveSpec>,
    parsed_items: ParsedItems,
    parse_mode: pob_parser::ParseMode,
    shared: Option<&BuildShared>,
    skills: bool,
    carry: Option<&crate::session::SkillsCarry>,
) -> Result<Env, InitEnvError> {
    let mut prof = profile::Marker::new();
    let opts = InitEnvMode { parse_mode };
    // --- (1) env + the three ModDBs + the actors (:490-547) ---------------------------------------------------------
    let own_lists;
    let (config, config_lists) = match shared {
        Some(sh) => (sh.config.clone(), &sh.config_lists),
        None => {
            let (config, lists) = load_config(&build, opts.parse_mode)?;
            own_lists = lists;
            (config, &own_lists)
        }
    };
    prof.mark("config");

    let mod_db = ModStore::new_db(None);
    let enemy_db = ModStore::new_db(None);
    let item_mod_db = ModStore::new_db(None);

    // :511 `build.configTab.enemyLevel or m_min(data.misc.MaxEnemyLevel, build.characterLevel)`: UpdateLevel always
    // leaves a number there (L4: even 0 would win), so the fallback is dead.
    let enemy_level = config.enemy_level;

    // :505 `env.classId = env.spec.curClassId` -- PassiveSpec's constructor always selects a class.
    let class_id = spec
        .cur_class_id
        .ok_or_else(|| InitEnvError("Modules/CalcSetup.lua:505: env.spec.curClassId is nil".into()))?;

    let player = new_actor(&mod_db, build.level);
    let enemy = new_actor(&enemy_db, enemy_level);
    // :513-523, in this order: both DBs point at their actor, the actors point at each other, and BOTH actor tables get a
    // `player` back-reference (what `ActorCondition` with `actor = "player"` follows out of the enemy DB).
    {
        let (p, e) = (player.borrow(), enemy.borrow());
        mod_db.borrow_mut().set_actor(&p.actor);
        enemy_db.borrow_mut().set_actor(&e.actor);
        p.actor.borrow_mut().actors.insert(Rc::from("enemy"), e.actor.clone());
        e.actor.borrow_mut().actors.insert(Rc::from("enemy"), p.actor.clone());
        e.actor.borrow_mut().actors.insert(Rc::from("player"), p.actor.clone());
        p.actor.borrow_mut().actors.insert(Rc::from("player"), p.actor.clone());
    }
    player.borrow_mut().enemy = Rc::downgrade(&enemy);
    enemy.borrow_mut().enemy = Rc::downgrade(&player);
    enemy.borrow_mut().player = Rc::downgrade(&player);
    player.borrow_mut().player = Rc::downgrade(&player);

    let calcs_input = build.calcs_input.clone();
    let mut env = Env {
        build: build.clone(),
        mode: "MAIN",
        build_breakdown: true,
        class_id,
        config,
        calcs_input,
        mod_db: mod_db.clone(),
        enemy_db: enemy_db.clone(),
        item_mod_db: item_mod_db.clone(),
        enemy_level,
        player: player.clone(),
        enemy: enemy.clone(),
        // --- (2) buff mode (:571-595): MAIN is always "EFFECTIVE" ------------------------------------------------------
        mode_buffs: true,
        mode_combat: true,
        mode_effective: true,
        alloc_nodes: IndexMap::new(),
        initial_node_mod_db: ModStore::new_list(None),
        keystones_added: IndexMap::new(),
        radius_jewel_list: Vec::new(),
        extra_radius_node_list: IndexMap::new(),
        granted_passives: IndexMap::new(),
        granted_skills: Vec::new(),
        granted_skills_nodes: Vec::new(),
        granted_skills_items: Vec::new(),
        granted_skills_extra: Vec::new(),
        granted_skills_items_extra: Vec::new(),
        explode_sources: Vec::new(),
        item_warnings: IndexMap::new(),
        flasks: Vec::new(),
        tinctures: Vec::new(),
        flask_slot_map: IndexMap::new(),
        flask_slot_occupied: IndexMap::new(),
        requirements_table_items: Vec::new(),
        requirements_table_gems: Vec::new(),
        requirements_table: Vec::new(),
        main_socket_group: 1.0,
        aegis_mod_list: None,
        the_iron_mass: None,
        weapon_mod_list1: None,
        node_granted_skills: IndexMap::new(),
        node_final_mod_list: IndexMap::new(),
        jewel_limit_disabled: Vec::new(),
        ordered_slots: Vec::new(),
        items: IndexMap::new(),
        skills: None,
        skill_mod_lists: Vec::new(),
        unimplemented: Vec::new(),
        limited_skills: None,
        session: std::rc::Weak::new(),
        spec,
    };
    // The config apply closures this port has no entry for (R3.5): visible, not silent.
    env.unimplemented.extend(config_lists.unimplemented.iter().cloned());
    let modes = (env.mode_buffs, env.mode_combat, env.mode_effective);

    // --- (3) classStats (:588) -----------------------------------------------------------------------------------------
    // `tree.characterData and tree.characterData[classId] or tree.classes[classId]` -- the 3.06 tree keeps the base
    // attributes in `characterData` (its classes have none, D19.5); every tree from 3.12 on puts them on the class.
    let (base_str, base_dex, base_int) = match env
        .spec
        .tree
        .json
        .character_data
        .as_ref()
        .and_then(|cd| cd.get(class_id.to_string().as_str()))
    {
        Some(cd) => (cd.base_str, cd.base_dex, cd.base_int),
        None => {
            let c = env.spec.tree.classes.get(class_id).ok_or_else(|| {
                InitEnvError(format!(
                    "Modules/CalcSetup.lua:588: no class {class_id} in tree {}",
                    env.spec.tree_version
                ))
            })?;
            (c.base_str, c.base_dex, c.base_int)
        }
    };

    // --- (4) the base modifier database (:590-650) --------------------------------------------------------------------
    for (stat, v) in [("Str", base_str), ("Dex", base_dex), ("Int", base_int)] {
        let v = v.ok_or_else(|| {
            InitEnvError(format!(
                "Modules/CalcSetup.lua:590: classStats[\"base_{}\"] is nil for class {class_id} of tree {}",
                stat.to_lowercase(),
                env.spec.tree_version
            ))
        })?;
        base(&mod_db, stat, ModType::Base, n(v), vec![]);
    }
    set_mult(&mod_db, "Level", max2(1.0, min2(100.0, build.level)));
    init_mod_db(modes, &mod_db);
    {
        use ModType::{Base, Flag, Inc, More};
        let db = &mod_db;
        let b = |name: &str, v: f64| base(db, name, Base, n(v), vec![]);
        let level_mult = |b: f64| tag(&[("type", s("Multiplier")), ("var", s("Level")), ("base", n(b))]);
        base(db, "Life", Base, n(cc("life_per_level")), vec![level_mult(38.0)]);
        base(db, "Mana", Base, n(cc("mana_per_level")), vec![level_mult(34.0)]);
        base(
            db,
            "ManaRegen",
            Base,
            n(misc("ManaRegenBase")),
            vec![tag(&[("type", s("PerStat")), ("stat", s("Mana")), ("div", n(1.0))])],
        );
        b("Devotion", 0.0);
        b("Evasion", cc("base_evasion_rating"));
        base(
            db,
            "Accuracy",
            Base,
            n(cc("accuracy_rating_per_level")),
            vec![level_mult(-cc("accuracy_rating_per_level"))],
        );
        b("CritMultiplier", cc("base_critical_strike_multiplier") - 100.0);
        base(
            db,
            "DotMultiplier",
            Base,
            n(cc("critical_ailment_dot_multiplier_+")),
            vec![cond("CriticalStrike")],
        );
        // L4: `env.configInput.resistancePenalty or -60` -- a configured 0 wins over the -60.
        let penalty = match env.config.input.get("resistancePenalty") {
            None | Some(InputValue::Bool(false)) => n(-60.0),
            Some(InputValue::Num(x)) => n(*x),
            Some(InputValue::Str(x)) => Value::str(x),
            Some(InputValue::Bool(true)) => Value::Bool(true),
        };
        for name in ["FireResist", "ColdResist", "LightningResist", "ChaosResist"] {
            base(db, name, Base, penalty.clone(), vec![]);
        }
        b("TotemFireResist", 40.0);
        b("TotemColdResist", 40.0);
        b("TotemLightningResist", 40.0);
        b("TotemChaosResist", 20.0);
        let attack = mod_flag("Attack");
        base(
            db,
            "CritChance",
            Inc,
            n(cc("critical_strike_chance_+%_per_power_charge")),
            vec![mult("PowerCharge")],
        );
        new_mod(
            db,
            "Speed",
            Inc,
            n(cc("base_attack_speed_+%_per_frenzy_charge")),
            Some("Base"),
            attack,
            0,
            vec![mult("FrenzyCharge")],
        );
        new_mod(
            db,
            "Speed",
            Inc,
            n(cc("base_cast_speed_+%_per_frenzy_charge")),
            Some("Base"),
            mod_flag("Cast"),
            0,
            vec![mult("FrenzyCharge")],
        );
        base(
            db,
            "Damage",
            More,
            n(cc("object_inherent_damage_+%_final_per_frenzy_charge")),
            vec![mult("FrenzyCharge")],
        );
        base(
            db,
            "PhysicalDamageReduction",
            Base,
            n(cc("physical_damage_reduction_%_per_endurance_charge")),
            vec![mult("EnduranceCharge")],
        );
        base(
            db,
            "ElementalDamageReduction",
            Base,
            n(cc("elemental_damage_reduction_%_per_endurance_charge")),
            vec![mult("EnduranceCharge")],
        );
        b("MaximumRage", cc("maximum_rage"));
        b("Multiplier:GaleForce", 0.0);
        b("MaximumGaleForce", 10.0);
        b("MaximumFortification", cc("base_max_fortification"));
        b("MaximumValour", 50.0);
        b("Multiplier:IntensityLimit", 3.0);
        let rampage = || {
            tag(&[
                ("type", s("Multiplier")),
                ("var", s("Rampage")),
                ("limit", n(cc("max_rampage_stacks") / 20.0)),
                ("div", n(20.0)),
            ])
        };
        base(db, "Damage", Inc, n(cc("damage_+%_per_10_rampage_stacks")), vec![rampage()]);
        base(
            db,
            "MovementSpeed",
            Inc,
            n(cc("movement_velocity_+%_per_10_rampage_stacks")),
            vec![rampage()],
        );
        b("ActiveTrapLimit", cc("base_number_of_traps_allowed"));
        b("ActiveMineLimit", cc("base_number_of_remote_mines_allowed"));
        b("MineThrowCount", 1.0);
        b("TrapThrowCount", 1.0);
        b("ActiveBrandLimit", 3.0);
        b("EnemyCurseLimit", 1.0);
        b("SocketedCursesHexLimitValue", 1.0);
        b("ProjectileCount", 1.0);
        let dw_speed = cc("dual_wield_inherent_attack_speed_+%_final");
        new_mod(
            db,
            "Speed",
            More,
            n(dw_speed),
            Some("Base"),
            attack,
            0,
            vec![cond("DualWielding"), cond_neg("DoubledInherentDualWieldingSpeed")],
        );
        new_mod(
            db,
            "Speed",
            More,
            n(2.0 * dw_speed),
            Some("Base"),
            attack,
            0,
            vec![cond("DualWielding"), cond("DoubledInherentDualWieldingSpeed")],
        );
        let dw_block = cc("inherent_block_while_dual_wielding_%");
        base(
            db,
            "BlockChance",
            Base,
            n(dw_block),
            vec![
                cond("DualWielding"),
                cond_neg("NoInherentBlock"),
                cond_neg("DoubledInherentDualWieldingBlock"),
            ],
        );
        base(
            db,
            "BlockChance",
            Base,
            n(2.0 * dw_block),
            vec![
                cond("DualWielding"),
                cond_neg("NoInherentBlock"),
                cond("DoubledInherentDualWieldingBlock"),
            ],
        );
        new_mod(
            db,
            "Damage",
            More,
            n(200.0),
            Some("Base"),
            0,
            keyword_flag("Bleed"),
            vec![
                tag(&[("type", s("ActorCondition")), ("actor", s("enemy")), ("var", s("Moving"))]),
                cond_neg("NoExtraBleedDamageToMovingEnemy"),
            ],
        );
        base(db, "Condition:BloodStance", Flag, Value::Bool(true), vec![cond_neg("SandStance")]);
        base(
            db,
            "Condition:PrideMinEffect",
            Flag,
            Value::Bool(true),
            vec![cond_neg("PrideMaxEffect")],
        );
        b(
            "PerBrutalTripleDamageChance",
            cc("chance_to_deal_triple_damage_%_per_brutal_charge"),
        );
        b("PerAfflictionAilmentDamage", cc("ailment_damage_+%_final_per_affliction_charge"));
        b(
            "PerAfflictionNonDamageEffect",
            cc("non_damaging_ailment_effect_+%_final_per_affliction_charge"),
        );
        b(
            "PerAbsorptionElementalEnergyShieldRecoup",
            cc("elemental_damage_taken_goes_to_energy_shield_over_4_seconds_%_per_absorption_charge"),
        );
        b("TinctureLimit", 1.0);
        base(db, "ManaDegenPercentTincture", Base, n(1.0), vec![mult("EffectiveManaBurnStacks")]);
        base(db, "LifeDegenPercentTincture", Base, n(1.0), vec![mult("WeepingWoundsStacks")]);
        b("PresenceRadius", cc("base_presence_radius"));
    }

    // --- (5) bandit (:651-660). D17: this reads configInput, NOT `<Build bandit=...>`. ------------------------------
    match env.config.input.get("bandit") {
        Some(InputValue::Str(b)) if b == "Alira" => {
            new_mod(&mod_db, "ElementalResist", ModType::Base, n(15.0), Some("Bandit"), 0, 0, vec![]);
        }
        Some(InputValue::Str(b)) if b == "Kraityn" => {
            new_mod(&mod_db, "MovementSpeed", ModType::Inc, n(8.0), Some("Bandit"), 0, 0, vec![]);
        }
        Some(InputValue::Str(b)) if b == "Oak" => {
            new_mod(&mod_db, "Life", ModType::Base, n(40.0), Some("Bandit"), 0, 0, vec![]);
        }
        _ => new_mod(&mod_db, "ExtraPoints", ModType::Base, n(1.0), Some("Bandit"), 0, 0, vec![]),
    }

    // --- (6) pantheons (:663-673). D17 again: "None" on every build with a <Config> section. --------------------------
    for key in ["pantheonMajorGod", "pantheonMinorGod"] {
        let god_name = match env.config.input.get(key) {
            Some(InputValue::Str(g)) => g.clone(),
            Some(InputValue::Num(x)) => pob_lua::tostring_num(*x),
            Some(InputValue::Bool(b)) => b.to_string(),
            None => "nil".into(),
        };
        if god_name == "None" {
            continue;
        }
        let god = pantheons()
            .get(god_name.as_str())
            .ok_or_else(|| InitEnvError(format!("Modules/PantheonTools.lua:7: data.pantheons[\"{god_name}\"] is nil")))?;
        apply_soul_mod(&mod_db, god, opts.parse_mode);
    }

    // --- (7) the enemy database (:676-678) -----------------------------------------------------------------------------
    init_mod_db(modes, &enemy_db);
    // `data.monsterAccuracyTable[env.enemyLevel]` -- a 1-based Lua sequence (L8).
    let accuracy = monster_table_at(&constants().monster_accuracy_table, enemy_level).ok_or_else(|| {
        InitEnvError(format!(
            "Modules/CalcSetup.lua:677: data.monsterAccuracyTable[{}] is nil",
            pob_lua::tostring_num(enemy_level)
        ))
    })?;
    base(&enemy_db, "Accuracy", ModType::Base, n(accuracy), vec![]);
    new_mod(
        &enemy_db,
        "Condition:AgainstDamageOverTime",
        ModType::Flag,
        Value::Bool(true),
        Some("Base"),
        mod_flag("Dot"),
        0,
        vec![tag(&[("type", s("ActorCondition")), ("actor", s("player")), ("var", s("Combat"))])],
    );

    // --- (8) the config tab's two lists (:681-685); the party tab's is empty ------------------------------------------
    mod_db.borrow_mut().add_list(&config_lists.mod_list.mods);
    enemy_db.borrow_mut().add_list(&config_lists.enemy_mod_list.mods);
    // :685 `enemyDB:AddList(build.partyTab.enemyModList)` -- the Party tab is not modelled; only a build whose XML carries
    // a <Party> section can lose anything here.
    if build.unknown_sections.iter().any(|s| s == "Party") {
        env.unimplemented
            .push("CalcSetup.lua:685 build.partyTab.enemyModList (<Party> section present, PartyTab is P5)".into());
    }

    // (9) `override.conditions` (:696-700) -- no override in MAIN.

    prof.mark("(1)-(9) env, base modDB, enemy, config lists");

    // --- (10) allocated nodes (:702-776) -------------------------------------------------------------------------------
    let allocated_notable_count = f64::from(env.spec.allocated_notable_count);
    let allocated_keystone_count = f64::from(env.spec.allocated_keystone_count);
    let allocated_mastery_count = f64::from(env.spec.allocated_mastery_count);
    let allocated_mastery_type_count = f64::from(env.spec.allocated_mastery_type_count);
    let allocated_mastery_types = env.spec.allocated_mastery_types.clone();

    // `copyTable(env.spec.allocNodes, true)` -- a shallow copy: the same nodes.
    env.alloc_nodes = env.spec.alloc_nodes.clone();
    let (initial, _) = build_mod_list_for_node_list(&mut env, true);
    env.initial_node_mod_db = initial.clone();
    merge_keystones(&mut env, &initial);

    // --- (11) the allocation multipliers (:778-797) -- deliberately WITHOUT a source ----------------------------------
    let alloc_mult = |name: &str, v: f64| new_mod(&mod_db, name, ModType::Base, n(v), None, 0, 0, vec![]);
    if allocated_notable_count > 0.0 {
        alloc_mult("Multiplier:AllocatedNotable", allocated_notable_count);
    }
    if allocated_keystone_count > 0.0 {
        alloc_mult("Multiplier:AllocatedKeystone", allocated_keystone_count);
    }
    if allocated_mastery_count > 0.0 {
        alloc_mult("Multiplier:AllocatedMastery", allocated_mastery_count);
    }
    if allocated_mastery_type_count > 0.0 {
        alloc_mult("Multiplier:AllocatedMasteryType", allocated_mastery_type_count);
    }
    if let Some(&life) = allocated_mastery_types.get("Life Mastery")
        && life > 0
    {
        alloc_mult("Multiplier:AllocatedLifeMastery", f64::from(life));
    }
    // :792-796 `allocatedTattooTypes` -- tattoos are P5; the table is always empty.

    prof.mark("(10)-(11) allocated nodes");

    // --- (12) items (:800-1370) --------------------------------------------------------------------------------------
    let item_set = active_item_set(&build);
    env.ordered_slots = build_ordered_slots(&env.spec, item_set);
    env.items = parsed_items;
    // `build.skillsTab` as the first BuildOutput finds it: SkillsTab:Load + UpdateSocketGroups (Build.lua:691), R6.2/R6.3.
    if skills {
        let loaded = match shared.map(|sh| &sh.skills_tab) {
            Some(Ok(tab)) => Ok(SkillsState::from_tab(&build, tab.clone())),
            Some(Err(e)) => Err(e.clone()),
            None => SkillsState::load(&build, &env.ordered_slots, &env.items),
        };
        let mut st = loaded.map_err(|e| InitEnvError(format!("SkillsTab:Load: {e}")))?;
        if let Some(c) = carry {
            st.from_item_marks.clone_from(&c.from_item_marks);
            st.cost_wiped.clone_from(&c.cost_wiped);
        }
        env.skills = Some(st);
    }
    prof.mark("(12) orderedSlots + SkillsTab:Load");
    let use_second_weapon_set = item_set.use_second_weapon_set;
    let mut items: IndexMap<String, Rc<Item>> = IndexMap::new();
    let mut jewel_limits: IndexMap<String, f64> = IndexMap::new();

    // An expansion socket (`expansionJewel.parent`) is dropped by `PassiveSpec:Init` and put back by
    // `BuildClusterJewelGraphs` (D16). In `CacheOnly` (pob-ts) the graphs are not built -- a jewel sitting in one is never
    // equipped here, nor are the nodes its cluster would generate. Said per item. In `Full` a socket still missing is
    // missing upstream too (its parent is not allocated) and the slot is skipped like there.
    for &(node_id, item_id) in &env.spec.jewels.0 {
        let in_spec = node_id >= 0.0 && node_id.fract() == 0.0 && env.spec.node(node_id as u32).is_some();
        if in_spec || item_id == 0.0 || opts.parse_mode == pob_parser::ParseMode::Full {
            continue;
        }
        let what = env
            .item(item_id)
            .map_or_else(|| format!("item {}", pob_lua::tostring_num(item_id)), |i| i.name.clone());
        env.unimplemented.push(format!(
            "PassiveSpec.lua:1111 cluster-jewel socket {} not in the spec: {what} is not equipped (BuildClusterJewelGraphs, P5)",
            pob_lua::tostring_num(node_id)
        ));
    }

    let slots = env.ordered_slots.clone();
    for slot in &slots {
        let mut slot_name = slot.slot_name.clone();
        if (slot_name == "Graft 1" || slot_name == "Graft 2") && !env.spec.tree_version.contains("3_27") {
            continue;
        }
        // :811 -- Ring 3 only exists with The Unseen Hand allocated.
        if slot_name == "Ring 3" && ok(initial.borrow().flag(None, &["AdditionalRingSlot"])) != Some(true) {
            continue;
        }
        let item = env.item(slot.sel_item_id).cloned();
        if let Some(item) = &item {
            // :824-832 `grantedSkill.nameSpec = skillData and skillData.name or nil` (looked up only on the skill path).
            for skill in &item.granted_skills {
                let name_spec = match &env.skills {
                    Some(st) => skill.skill_id.as_str().and_then(|id| st.skills.get(id)).map(|ge| ge.name.clone()),
                    None => None,
                };
                env.granted_skills_items_extra.push(GrantedSkillExtra {
                    name_spec,
                    no_supports: skill.no_supports.clone(),
                    triggered: skill.triggered.clone(),
                    trigger_chance: skill.trigger_chance.clone(),
                });
                env.granted_skills_items.push(GrantedSkill {
                    skill_id: skill.skill_id.clone(),
                    level: skill.level.clone(),
                    source: skill.source.clone(),
                    slot_name: Some(slot_name.clone()),
                    source_item: Some(item.clone()),
                    source_node: None,
                });
            }
            if let Some(bml) = &item.base_mod_list
                && ok(temp_list(bml).borrow().flag(None, &["CanExplode"])) == Some(true)
            {
                env.explode_sources.push(ExplodeSource::Item(ByRef(item.clone())));
            }
        }
        // :838-843 weapon-set filter, then " Swap" is stripped off the NAME (not off the slot).
        let want_set = if use_second_weapon_set { 2.0 } else { 1.0 };
        if slot.weapon_set.is_some_and(|w| w != want_set) {
            continue;
        }
        if slot.weapon_set == Some(2.0) && use_second_weapon_set {
            slot_name = slot_name.replace(" Swap", "");
        }

        if let Some(node_id) = slot.node_id {
            // :845-847 -- an unallocated socket holds nothing.
            if !env.alloc_nodes.contains_key(&node_id) {
                continue;
            }
            if let Some(item) = &item {
                let item_id = item.id.unwrap_or(0.0);
                env.jewel_limit_disabled.retain(|&i| i != item_id);
                let jd = item.jewel_data.as_ref();
                if item.ty.as_deref() == Some("Jewel") && item.name.contains("The Adorned, Crimson Jewel") {
                    let jd_num = |k: &str| jd.and_then(|d| d.num(k));
                    if let Some(magic) = jd_num("corruptedMagicJewelIncEffect") {
                        set_mult(&mod_db, "CorruptedMagicJewelEffect", magic / 100.0);
                    }
                    if let Some(rare) = jd_num("corruptedRareJewelIncEffect") {
                        set_mult(&mod_db, "CorruptedRareJewelEffect", rare / 100.0);
                    }
                }
                // :862-870 jewel limits (L4: a `limit` of 0 is still truthy).
                if let Some(limit) = item.limit
                    && !input_truthy(&env.config, "ignoreJewelLimits")
                {
                    let limit_key = if item.base.and_then(|b| b.sub_type.as_deref()) == Some("Timeless") {
                        "Historic".to_owned()
                    } else {
                        item.title.clone().unwrap_or_default()
                    };
                    let used = jewel_limits.get(&limit_key).copied();
                    if used.is_some_and(|u| u >= limit) {
                        env.jewel_limit_disabled.push(item_id);
                        env.item_warnings.entry("jewelLimitWarning".into()).or_default().push(limit_key);
                        continue;
                    }
                    jewel_limits.insert(limit_key, used.unwrap_or(0.0) + 1.0);
                }
                // :871-935 -- gated on `item.jewelRadiusIndex` (`override` is nil in MAIN: no `extraJewelFuncs`, see (16)).
                // `ParseMode::CacheOnly` reproduces pob-ts, which has no radius jewels.
                if let Some(radius_index) = item.jewel_radius_index {
                    if opts.parse_mode == pob_parser::ParseMode::Full {
                        add_radius_jewel(&mut env, item, node_id, radius_index);
                    } else {
                        env.unimplemented
                            .push(format!("CalcSetup.lua:871 radius jewel {} (nodesInRadius, pob-ts)", item.name));
                    }
                }
            }
        }
        // :936-938 -- kept even if the slot is later disabled.
        if let Some(item) = &item
            && item.ty.as_deref() == Some("Flask")
            && item.base.and_then(|b| b.sub_type.as_deref()) == Some("Life")
            && let Some(fd) = &item.flask_data
        {
            let cur = item_mod_db.borrow().multipliers.get("LifeFlaskRecovery").copied().unwrap_or(0.0);
            let total = fd.num("lifeTotal").unwrap_or(0.0);
            set_mult(&item_mod_db, "LifeFlaskRecovery", max2(cur, total));
        }
        if let Some(item) = item {
            items.insert(slot_name, item);
        }
    }

    // :943-1007 -- items that disable other items (Kitava's Thirst, tinctures, ...) --------------------------------------
    if !input_truthy(&env.config, "ignoreItemDisablers") {
        let mut item_disabled: IndexMap<String, String> = IndexMap::new();
        let mut item_disablers: IndexMap<String, String> = IndexMap::new();
        let read_disables = |mods: &[Rc<Mod>],
                             key: &str,
                             items: &IndexMap<String, Rc<Item>>,
                             disablers: &mut IndexMap<String, String>,
                             disabled: &mut IndexMap<String, String>| {
            for m in mods {
                for t in &m.tags {
                    if !t.ty().is_str("DisablesItem") {
                        continue;
                    }
                    let exclude = t.get("excludeItemType");
                    // `tag.slotName` as a table key; the data only has strings here.
                    let target = match t.get("slotName") {
                        Value::Str(x) => x.to_string(),
                        Value::Num(x) => pob_lua::tostring_num(x),
                        other => panic!("Modules/CalcSetup.lua:975: DisablesItem tag with slotName {other:?}"),
                    };
                    if exclude.truthy()
                        && items
                            .get(&target)
                            .is_some_and(|i| exclude.as_str().is_some_and(|e| i.ty.as_deref() == Some(e)))
                    {
                        break;
                    }
                    disablers.insert(key.to_owned(), target.clone());
                    disabled.insert(target, key.to_owned());
                    break;
                }
            }
        };
        // :948-958 -- tree first; `"Flag"` is upstream's typo'd mod type (L23), not "FLAG".
        let tree_entries = ok(initial
            .borrow()
            .tabulate(Some(ModType::FlagTypo), Some(&cfg_source("Tree")), &["CanNotUseItem"]));
        for e in tree_entries {
            let key = e.mod_.source.as_deref().unwrap_or("nil").to_owned();
            read_disables(std::slice::from_ref(&e.mod_), &key, &items, &mut item_disablers, &mut item_disabled);
        }
        for slot in &slots {
            let Some(disabling) = items.get(&slot.slot_name).cloned() else {
                continue;
            };
            let src = src_list_of(&disabling, slot.slot_num).unwrap_or_else(|| {
                panic!(
                    "Modules/CalcSetup.lua:964: bad argument #1 to 'ipairs' (table expected, got nil) -- {} has neither modList nor slotModList[slotNum]",
                    slot.slot_name
                )
            });
            read_disables(&src, &slot.slot_name, &items, &mut item_disablers, &mut item_disabled);
        }
        // :983-1004 -- walk each chain to its start, then disable every other link. `pairs(itemDisablers)` is a hash order
        // upstream; this port walks insertion order (as pob-ts). No golden has a disabler.
        let mut visited: Vec<String> = Vec::new();
        let mut true_disabled: Vec<String> = Vec::new();
        let starts: Vec<String> = item_disablers.keys().cloned().collect();
        for start in starts {
            if visited.contains(&start) {
                continue;
            }
            let mut cur = start;
            // :986 `local curChain = { slot = true }` seeds the LITERAL key "slot", not the variable's value (D19.2) -- the
            // cycle guard only starts working one hop in. Kept verbatim.
            let mut cur_chain: Vec<String> = vec!["slot".into()];
            while let Some(next) = item_disabled.get(&cur) {
                cur = next.clone();
                if cur_chain.contains(&cur) {
                    break;
                }
                cur_chain.push(cur.clone());
            }
            // `repeat ... until (not slot or visited[slot])`
            loop {
                visited.push(cur.clone());
                let Some(next) = item_disablers.get(&cur) else { break };
                cur = next.clone();
                visited.push(cur.clone());
                true_disabled.push(cur.clone());
                match item_disablers.get(&cur) {
                    Some(after) if !visited.contains(after) => cur = after.clone(),
                    _ => break,
                }
            }
        }
        for slot_name in true_disabled {
            items.shift_remove(&slot_name);
        }
    }

    // :1009-1022 anoint warnings -- `getMissingAnointCount` only feeds `env.itemWarnings` (UI).
    env.unimplemented
        .push("CalcSetup.lua:1011 getMissingAnointCount / missingAnointWarning (UI only)".into());

    // :1025-1370 -- flask bookkeeping, then every item's mods into itemModDB -----------------------------------------------
    let mut class_restrictions: IndexMap<String, String> = IndexMap::new();
    for slot in &slots {
        let slot_name = slot.slot_name.clone();
        let mut item = items.get(&slot_name).cloned();
        if let Some(it) = item.clone()
            && it.ty.as_deref() == Some("Flask")
        {
            set_cond(
                &item_mod_db,
                &format!("Have{}", no_whitespace(it.base_name.as_deref().unwrap_or("nil"))),
                true,
            );
            if slot.active == Some(true) {
                env.flasks.push(it.clone());
            }
            // `tonumber(slotName:match("Flask (%d+)"))`
            if let Some(num) = flask_num(&slot_name) {
                env.flask_slot_map.insert(slot_name.clone(), num);
                env.flask_slot_occupied.insert(pob_lua::tostring_num(num), true);
            }
            if it.base.and_then(|b| b.sub_type.as_deref()) == Some("Life") {
                let highest = item_mod_db.borrow().multipliers.get("LifeFlaskCharges").copied().unwrap_or(0.0);
                let charges_max = it
                    .flask_data
                    .as_ref()
                    .and_then(|f| f.num("chargesMax"))
                    .expect("Modules/CalcSetup.lua:1040: attempt to compare nil with number (flaskData.chargesMax)");
                if charges_max > highest {
                    set_mult(&item_mod_db, "LifeFlaskCharges", charges_max);
                }
            }
            item = None;
        } else if let Some(it) = item.clone()
            && it.ty.as_deref() == Some("Tincture")
        {
            if slot.active == Some(true) {
                env.tinctures.push(it.clone());
            }
            item = None;
        }
        let mut scale = 1.0;
        if let Some(it) = &item
            && it.ty.as_deref() == Some("Jewel")
            && it.base.and_then(|b| b.sub_type.as_deref()) == Some("Abyss")
            && slot.parent_slot.is_some()
        {
            // :1052-1060 -- abyssal sockets are P5; nothing ever lands in one here.
            env.unimplemented
                .push("CalcSetup.lua:1052 abyss jewel in an abyssal socket (P5)".into());
            item = None;
        }
        if slot.node_id.is_some()
            && let Some(it) = &item
            && it.ty.as_deref() == Some("Jewel")
            && it
                .jewel_data
                .as_ref()
                .is_some_and(|d| d.get("jewelIncEffectFromClassStart").truthy())
        {
            // :1061-1066 -- needs `node.distanceToClassStart` (PassiveSpec path machinery, P5).
            env.unimplemented.push(format!(
                "CalcSetup.lua:1061 jewelIncEffectFromClassStart on {} (distanceToClassStart, P5)",
                it.name
            ));
        }
        let Some(item) = item else { continue };

        player.borrow_mut().item_list.insert(slot_name.clone(), item.clone());
        let src_list = src_list_of(&item, slot.slot_num).unwrap_or_default();
        // `if item.requirements then` -- a parsed item always carries the table.
        env.requirements_table_items.push(ReqEntry {
            source: "Item",
            source_item: Some(item.clone()),
            source_slot: Some(slot_name.clone()),
            source_gem: None,
            str_: item.requirements.str_mod,
            dex: item.requirements.dex_mod,
            int: item.requirements.int_mod,
        });

        // --- :1100-1270 the special cases, in upstream order ---------------------------------------------------------
        let ty = item.ty.as_deref();
        let sub_type = item.base.and_then(|b| b.sub_type.as_deref());
        let gloves_mod = || ok(calc_lib::mod_(&initial.borrow(), None, &["EffectOfBonusesFromGloves"]));
        let boots_mod = || ok(calc_lib::mod_(&initial.borrow(), None, &["EffectOfBonusesFromBoots"]));
        let quiver_inc = || ok(initial.borrow().sum(ModType::Inc, None, &["EffectOfBonusesFromQuiver"]));
        let corrupted_jewel = || {
            let key = format!("Corrupted{}JewelEffect", title_case_rarity(&item.rarity));
            // `and not env.spec.nodes[slot.nodeId].containJewelSocket`: READ in three places upstream, WRITTEN in none
            // (D19.1) -- always nil, so the clause is always true. A multiplier of 0 is truthy (L4): `contains_key`.
            mod_db.borrow().multipliers.contains_key(key.as_str())
                && ty == Some("Jewel")
                && item.corrupted
                && slot.node_id.is_some()
                && sub_type != Some("Charm")
        };
        if ty == Some("Shield")
            && env
                .alloc_nodes
                .get(&45175)
                .is_some_and(|&i| env.spec.nodes[i].dn() == "Necromantic Aegis")
        {
            let aegis = ModStore::new_list(None);
            env.aegis_mod_list = Some(aegis.clone());
            for m in &src_list {
                if !has_socketed_in(m) {
                    ok(aegis.borrow_mut().scale_add_mod(m, scale, false));
                } else {
                    ok(item_mod_db.borrow_mut().scale_add_mod(m, scale, false));
                }
            }
        } else if (slot_name == "Weapon 1" || slot_name == "Weapon 2")
            && mod_db.borrow().conditions.get("AffectedByEnergyBlade").copied().unwrap_or(false)
        {
            // :1118-1152 -- only reachable through the skill phase's `override.conditions` (P5).
            env.unimplemented
                .push("CalcSetup.lua:1118 Energy Blade weapon replacement (needs the skill phase, P5)".into());
            ok(item_mod_db.borrow_mut().scale_add_list(&src_list, scale, false));
        } else if slot_name == "Weapon 1" && item.name == "The Iron Mass, Gladius" {
            let iron = ModStore::new_list(None);
            env.the_iron_mass = Some(iron.clone());
            for m in &src_list {
                if !has_socketed_in(m) {
                    ok(iron.borrow_mut().scale_add_mod(m, scale, false));
                }
                ok(item_mod_db.borrow_mut().scale_add_mod(m, scale, false));
            }
        } else if slot_name == "Weapon 1"
            && item
                .granted_skills
                .first()
                .is_some_and(|g| g.skill_id.is_str("UniqueAnimateWeapon"))
        {
            let wml = ModStore::new_list(None);
            env.weapon_mod_list1 = Some(wml.clone());
            for m in &src_list {
                if !has_socketed_in(m) {
                    ok(wml.borrow_mut().scale_add_mod(m, scale, false));
                } else {
                    ok(item_mod_db.borrow_mut().scale_add_mod(m, scale, false));
                }
            }
        } else if item.name.contains("Kalandra's Touch") {
            // :1189 `item.name:match("Kalandra's Touch")` -- no magic characters, a plain find.
            let other = match slot_name.as_str() {
                "Ring 1" => items.get("Ring 2").cloned(),
                "Ring 2" => items.get("Ring 1").cloned(),
                _ => None,
            };
            if let Some(other) = other
                && !other.name.contains("Kalandra's Touch")
            {
                for m in src_list_of(&other, slot.slot_num).unwrap_or_default() {
                    if has_socketed_in(&m) {
                        continue;
                    }
                    // `copyTable(mod)` + setSource on the copy (R4).
                    let mut c = m.deep_copy();
                    pob_mods::format::set_source(&mut c, item.mod_source.as_deref().unwrap_or("nil"));
                    ok(item_mod_db.borrow_mut().scale_add_mod(&Rc::new(c), scale, false));
                }
                for (mult_name, property) in INFLUENCE_MULTS {
                    if item_has(&other, property) {
                        bump_mult(&item_mod_db, mult_name, 1.0);
                        bump_mult(&item_mod_db, &format!("Non{mult_name}"), -1.0);
                    }
                }
                if item_has(&other, "elder") || item_has(&other, "shaper") {
                    bump_mult(&item_mod_db, "ShaperOrElderItem", 1.0);
                }
                bump_mult(
                    &item_mod_db,
                    &format!("{}Equipped", no_spaces(other.base_name.as_deref().unwrap_or("nil"))),
                    1.0,
                );
            }
            // :1219-1223 -- only ExtraSkill implicits carry over.
            for m in &src_list {
                if &*m.name == "ExtraSkill" {
                    ok(item_mod_db.borrow_mut().scale_add_mod(m, scale, false));
                }
            }
        } else if ty == Some("Quiver") && (items.get("Weapon 1").is_some_and(|w| w.name.contains("Widowhail")) || quiver_inc() > 0.0) {
            let weapon1 = items.get("Weapon 1").cloned();
            // :1229 -- the `or 100` never fires: the `and` chain yields a number whenever Weapon 1 exists.
            let inc = match &weapon1 {
                Some(w) => {
                    let bml = temp_list(w.base_mod_list.as_deref().unwrap_or_default());
                    ok(bml.borrow().sum(ModType::Inc, None, &["EffectOfBonusesFromQuiver"])) + quiver_inc()
                }
                None => 100.0,
            };
            let widow_hail_mod = 1.0 + inc / 100.0;
            scale *= widow_hail_mod;
            new_mod(
                &mod_db,
                "WidowHailMultiplier",
                ModType::Base,
                n(widow_hail_mod),
                Some("Widowhail"),
                0,
                0,
                vec![],
            );
            let combined = ModStore::new_list(None);
            for m in &src_list {
                ok(combined.borrow_mut().merge_mod(m.clone(), false));
            }
            ok(item_mod_db.borrow_mut().scale_add_list(&list_mods(&combined), scale, false));
        } else if corrupted_jewel() {
            let key = format!("Corrupted{}JewelEffect", title_case_rarity(&item.rarity));
            scale += mod_db.borrow().multipliers[key.as_str()];
            let combined = ModStore::new_list(None);
            for m in &src_list {
                ok(combined.borrow_mut().merge_mod(m.clone(), false));
            }
            ok(item_mod_db.borrow_mut().scale_add_list(&list_mods(&combined), scale, false));
        } else if ty == Some("Gloves") && gloves_mod() != 1.0 {
            scale = gloves_mod() - 1.0;
            item_mod_db.borrow_mut().add_list(&merge_scaled(&src_list, scale));
        } else if ty == Some("Boots") && boots_mod() != 1.0 {
            scale = boots_mod() - 1.0;
            item_mod_db.borrow_mut().add_list(&merge_scaled(&src_list, scale));
        } else {
            ok(item_mod_db.borrow_mut().scale_add_list(&src_list, scale, false));
        }

        // :1272-1274 -- a class-restricted unique sets a condition named after itself, VALUED the class name (a string).
        // `ModStore.conditions` holds booleans (pob-mods), so the condition stores `true` (same truthiness for EvalMod) and
        // the string is kept aside for the Forbidden Flame/Flesh comparison below.
        if let Some(cr) = &item.class_restriction {
            let key = no_spaces(item.title.as_deref().unwrap_or("nil"));
            set_cond(&item_mod_db, &key, true);
            class_restrictions.insert(key.clone(), cr.clone());
            env.unimplemented.push(format!(
                "CalcSetup.lua:1273 itemModDB.conditions[\"{key}\"] = \"{cr}\" stored as true (ModStore.conditions is boolean)"
            ));
        }

        // --- :1275-1345 the item counters -----------------------------------------------------------------------------
        if !matches!(ty, Some("Jewel" | "Flask" | "Tincture" | "Graft")) {
            let key = match item.rarity.as_str() {
                "UNIQUE" | "RELIC" => {
                    if item.foulborn {
                        bump_mult(&item_mod_db, "FoulbornUniqueItem", 1.0);
                    }
                    "UniqueItem"
                }
                "RARE" => "RareItem",
                "MAGIC" => "MagicItem",
                _ => "NormalItem",
            };
            bump_mult(&item_mod_db, key, 1.0);
            set_cond(&item_mod_db, &format!("{key}In{slot_name}"), true);
            for (mult_name, property) in INFLUENCE_MULTS {
                if item_has(&item, property) {
                    bump_mult(&item_mod_db, mult_name, 1.0);
                } else {
                    bump_mult(&item_mod_db, &format!("Non{mult_name}"), 1.0);
                }
            }
            if item_has(&item, "shaper") || item_has(&item, "elder") {
                bump_mult(&item_mod_db, "ShaperOrElderItem", 1.0);
            }
            // `item.type:gsub(" ", ""):gsub(".+Handed", "")` -- "One Handed Sword" -> "Sword".
            let type_key = lua_gsub(&no_spaces(ty.unwrap_or("nil")), ".+Handed", "");
            bump_mult(&item_mod_db, &format!("{type_key}Item"), 1.0);
            if ty == Some("Ring") {
                bump_mult(
                    &item_mod_db,
                    &format!("{}Equipped", no_spaces(item.base_name.as_deref().unwrap_or("nil"))),
                    1.0,
                );
            }

            // :1308-1356 socket counts. `socketedGems` is filled from the socket groups of THIS slot whose gems carry
            // `gem.gemData` (R6.3); without the skill phase it is empty: `Socketed*GemsIn*` are 0 and every socket counts as
            // empty (the `gems` class of the "= pob-ts" comparisons, excluded by name).
            let socketed_gems = env.skills.as_ref().map(|st| st.socketed_gems(&slot_name)).unwrap_or_default();
            let mut empty = [0.0f64; 4]; // R, G, B, W
            let mut slot_gem_sockets_count = 0.0;
            let socketed_gems_len = socketed_gems.len() as f64;
            for (index, socket) in item.sockets.iter().enumerate() {
                // `for i, socket in ipairs(item.sockets)`; abyssal sockets ("A") are not gem sockets.
                let ci = match socket.color.as_str() {
                    "R" => 0,
                    "G" => 1,
                    "B" => 2,
                    "W" => 3,
                    _ => continue,
                };
                slot_gem_sockets_count += 1.0;
                if (index + 1) as f64 > socketed_gems_len {
                    empty[ci] += 1.0;
                }
            }
            // :1332-1343 only gems that fit in the item's sockets contribute to the colour multipliers
            let mut socketed_colours = [0.0f64; 3]; // R, G, B
            for g in socketed_gems.iter().take(min2(slot_gem_sockets_count, socketed_gems_len) as usize) {
                let tag = |t: &str| g.tags.get(t).copied().unwrap_or(false);
                for (i, t) in ["strength", "dexterity", "intelligence"].iter().enumerate() {
                    if tag(t) {
                        socketed_colours[i] += 1.0;
                    }
                }
            }
            set_mult(
                &item_mod_db,
                &format!("SocketedGemsIn{slot_name}"),
                min2(slot_gem_sockets_count, socketed_gems_len),
            );
            for (i, colour) in ["Red", "Green", "Blue"].iter().enumerate() {
                set_mult(&item_mod_db, &format!("Socketed{colour}GemsIn{slot_name}"), socketed_colours[i]);
            }
            set_mult(
                &item_mod_db,
                &format!("EmptySocketIn{slot_name}"),
                min2(slot_gem_sockets_count, empty[0] + empty[1] + empty[2] + empty[3]),
            );
            bump_mult(&item_mod_db, "EmptyRedSocketsInAnySlot", empty[0]);
            bump_mult(&item_mod_db, "EmptyGreenSocketsInAnySlot", empty[1]);
            bump_mult(&item_mod_db, "EmptyBlueSocketsInAnySlot", empty[2]);
            bump_mult(&item_mod_db, "EmptyWhiteSocketsInAnySlot", empty[3]);
            // :1351-1355 warn if socketed gems over socket limit
            if socketed_gems_len > slot_gem_sockets_count {
                env.item_warnings
                    .entry("socketLimitWarning".into())
                    .or_default()
                    .push(slot_name.clone());
            }
        }
    }
    if env.skills.is_none() {
        env.unimplemented
            .push("CalcSetup.lua:1318 socketedGems needs gem.gemData (skills off): Socketed*/Empty*Socket* multipliers".into());
    }

    // :1364-1367 -- the config overrides, `or` so a 0 still wins (L4).
    for (colour, key) in [
        ("Red", "overrideEmptyRedSockets"),
        ("Green", "overrideEmptyGreenSockets"),
        ("Blue", "overrideEmptyBlueSockets"),
        ("White", "overrideEmptyWhiteSockets"),
    ] {
        let name = format!("Empty{colour}SocketsInAnySlot");
        match env.config.input.get(key) {
            None | Some(InputValue::Bool(false)) => {}
            Some(InputValue::Num(x)) => set_mult(&item_mod_db, &name, *x),
            Some(other) => panic!("Modules/CalcSetup.lua:1364: configInput.{key} = {other:?} is not a number"),
        }
    }

    prof.mark("(12) items");

    // --- (13) mergeDB (:1385) ------------------------------------------------------------------------------------------
    merge_db(&mod_db, &item_mod_db);

    // --- (14) granted passives (:1388-1400) ----------------------------------------------------------------------------
    // `pairs(env.modDB:List(nil, "GrantedPassive"))` -- List returns a sequence, so `pairs` walks it in order.
    for passive in ok(mod_db.borrow().list(None, &["GrantedPassive"])) {
        let name = match &passive {
            Value::Str(x) => x.to_string(),
            other => format!("{other:?}"),
        };
        let tree_idx = env
            .spec
            .tree
            .notable_map
            .get(&name)
            .or_else(|| env.spec.tree.ascendancy_map.get(&name))
            .copied();
        let Some(tree_idx) = tree_idx else {
            // `build.latestTree.ascendancyMap[passive]` -- a second tree version, P5.
            env.unimplemented.push(format!(
                "CalcSetup.lua:1392 GrantedPassive \"{name}\" not in this tree (latestTree fallback, P5)"
            ));
            continue;
        };
        let id = env.spec.tree.nodes[tree_idx].id;
        let spec_idx = env
            .spec
            .node_idx(id)
            .unwrap_or_else(|| panic!("Modules/CalcSetup.lua:1391: spec.nodes[{id}] is nil"));
        env.alloc_nodes.insert(id, spec_idx);
        env.granted_passives.insert(id, true);
        env.extra_radius_node_list.shift_remove(&id);
    }
    // :1403-1418 Forbidden Flame/Flesh. `pairs` over the List sequence = in order.
    let mut matched_name: IndexMap<String, (Value, bool)> = IndexMap::new();
    for asc in ok(mod_db.borrow().list(None, &["GrantedAscendancyNode"])) {
        let name = match asc.get("name") {
            Value::Str(x) => x.to_string(),
            other => format!("{other:?}"),
        };
        let side = asc.get("side");
        let prev = matched_name.get(&name).cloned();
        if let Some((prev_side, false)) = prev
            && !prev_side.raw_eq(&side)
        {
            matched_name.insert(name.clone(), (prev_side, true));
            if let Some(&tree_idx) = env.spec.tree.ascendancy_map.get(&name) {
                let cls = &env.spec.cur_class_name;
                if class_restrictions.get("ForbiddenFlesh") == Some(cls) && class_restrictions.get("ForbiddenFlame") == Some(cls) {
                    let id = env.spec.tree.nodes[tree_idx].id;
                    // `env.allocNodes[node.id] = node` -- the TREE node upstream; the spec node here (same mods).
                    if let Some(spec_idx) = env.spec.node_idx(id) {
                        env.alloc_nodes.insert(id, spec_idx);
                        env.granted_passives.insert(id, true);
                    }
                }
            } else {
                env.unimplemented.push(format!(
                    "CalcSetup.lua:1408 GrantedAscendancyNode \"{name}\" not in this tree (latestTree fallback, P5)"
                ));
            }
        } else {
            matched_name.insert(name, (side, false));
        }
    }

    // --- (15) the allocated nodes' mods, into modDB (:1420-1425) -------------------------------------------------------
    {
        let (node_mod_list, node_explode) = build_mod_list_for_node_list(&mut env, true);
        mod_db.borrow_mut().add_list(&list_mods(&node_mod_list));
        // `env.explodeSources = tableConcat(explodeSources, env.explodeSources)` -- nodes first.
        let mut all: Vec<ExplodeSource> = node_explode.into_iter().map(ExplodeSource::Node).collect();
        all.append(&mut env.explode_sources);
        env.explode_sources = all;
    }

    // --- (16) ExtraJewelFunc (:1426-1437) ------------------------------------------------------------------------------
    let extra_jewel_funcs = ok(mod_db.borrow().tabulate(Some(ModType::List), None, &["ExtraJewelFunc"]));
    if !extra_jewel_funcs.is_empty() {
        env.unimplemented.push(format!(
            "CalcSetup.lua:1434 {} ExtraJewelFunc mods (initEnv would recurse; P5)",
            extra_jewel_funcs.len()
        ));
    }

    // --- (17) skills granted by tree nodes (:1439-1451) ----------------------------------------------------------------
    // `pairs(env.allocNodes)` -- insertion order here (D13, see build_mod_list_for_node_list).
    let alloc_ids: Vec<u32> = env.alloc_nodes.keys().copied().collect();
    for id in alloc_ids {
        let skills = env.node_granted_skills.get(&id).cloned().unwrap_or_default();
        for mut skill in skills {
            skill.source_node = Some(id);
            env.granted_skills_nodes.push(skill);
        }
    }
    env.granted_skills = env
        .granted_skills_nodes
        .iter()
        .chain(env.granted_skills_items.iter())
        .cloned()
        .collect();
    env.granted_skills_extra = std::iter::repeat_n(GrantedSkillExtra::default(), env.granted_skills_nodes.len())
        .chain(env.granted_skills_items_extra.iter().cloned())
        .collect();

    prof.mark("(13)-(17) mergeDB, granted passives, node mods");

    // --- (18) the skill phase (:1453-1970) ------------------------------------------------------------------------------
    // (18-) the merge of the granted skills (:1448-1563, R6.3)
    if let Some(st) = env.skills.as_mut() {
        st.merge_granted(&env.granted_skills, &env.granted_skills_extra, &env.explode_sources)
            .map_err(InitEnvError)?;
    } else {
        env.unimplemented
            .push("CalcSetup.lua:1453-1946 skills: socket groups, supports, createActiveSkill, buildActiveSkillModList (P5)".into());
    }

    prof.mark("(18-) granted-skill merge");

    // (18a) weapon data (:1565-1581)
    let weapon1 = player.borrow().item_list.get("Weapon 1").cloned();
    let weapon2 = player.borrow().item_list.get("Weapon 2").cloned();
    let gloves = player.borrow().item_list.contains_key("Gloves");
    let unarmed_rec = rules()
        .unarmed_weapon_data_for(class_id as f64)
        .ok_or_else(|| InitEnvError(format!("Modules/CalcSetup.lua:1566: data.unarmedWeaponData[{class_id}] is nil")))?;
    // `copyTable(data.unarmedWeaponData[classId])` -- the Lua record (the converter's `classId` key is not part of it).
    let unarmed = || {
        let mut d = DataTable::default();
        d.set("type", Value::str(&unarmed_rec.ty));
        d.set("AttackRate", n(unarmed_rec.AttackRate));
        d.set("CritChance", n(unarmed_rec.CritChance));
        d.set("PhysicalMin", n(unarmed_rec.PhysicalMin));
        d.set("PhysicalMax", n(unarmed_rec.PhysicalMax));
        d
    };
    let wd = |item: &Option<Rc<Item>>, k: f64| -> Option<DataTable> { item.as_ref()?.weapon_data.as_ref()?.get(&NumKey(k)).cloned() };
    let weapon_data1 = wd(&weapon1, 1.0).unwrap_or_else(unarmed);
    let weapon_data2 = if weapon_data1.get("countsAsDualWielding").truthy() {
        // `itemList["Weapon 1"].weaponData[2]` -- nil would make every later read fail upstream.
        wd(&weapon1, 2.0).unwrap_or_else(|| panic!("Modules/CalcSetup.lua:1568: Weapon 1 countsAsDualWielding without weaponData[2]"))
    } else if weapon2.is_none() {
        // Hollow Palm Technique (:1569-1578)
        let mut hollow = None;
        if weapon1.is_none() && !gloves {
            let keystones = match &mod_db.borrow().kind {
                pob_mods::StoreKind::Db(db) => db.mods.get("Keystone").cloned().unwrap_or_default(),
                pob_mods::StoreKind::List(_) => unreachable!(),
            };
            for k in keystones {
                if k.value.is_str("Hollow Palm Technique") {
                    hollow = Some(unarmed());
                    break;
                }
            }
        }
        hollow.unwrap_or_default()
    } else {
        wd(&weapon2, 2.0).unwrap_or_default()
    };
    {
        let mut p = player.borrow_mut();
        let mut a = p.actor.borrow_mut();
        a.weapon_data1 = Some(Rc::new(data_table_to_table(&weapon_data1)));
        a.weapon_data2 = Some(Rc::new(data_table_to_table(&weapon_data2)));
        a.item_list = Some(
            p.item_list
                .iter()
                .map(|(k, v)| (Rc::from(k.as_str()), v.clone() as Rc<dyn pob_mods::ItemLike>))
                .collect(),
        );
        drop(a);
        p.weapon_data1 = weapon_data1;
        p.weapon_data2 = weapon_data2;
    }

    // (18b) the main socket group (:1587-1590). Without the skill phase `#socketGroupList` is the XML's own group count (the
    // granted-skill groups are not appended).
    if let Some(st) = env.skills.as_mut() {
        env.main_socket_group = crate::skills::main_socket_group(st);
    } else {
        let groups = active_skill_groups(&build).len() as f64;
        env.main_socket_group = min2(max2(groups, 1.0), build.main_socket_group);
    }

    prof.mark("(18a-b) weapon data, main group");

    // (18c) supports, active skills, the main skill (:1593-1940, R6.3)
    if let Some(mut st) = env.skills.take() {
        let affected_by_energy_blade = mod_db.borrow().conditions.get("AffectedByEnergyBlade").copied().unwrap_or(false);
        let out = {
            let p = player.borrow();
            let view =
                |d: &DataTable| -> (Option<String>, bool) { (d.get("type").as_str().map(str::to_owned), d.get("countsAsAll1H").truthy()) };
            let (t1, a1) = view(&p.weapon_data1);
            let (t2, a2) = view(&p.weapon_data2);
            let inp = crate::skills::SetupInputs {
                build: &build,
                mod_db: &mod_db,
                ordered_slots: &env.ordered_slots,
                item_list: &p.item_list,
                weapon1: crate::calc_tools::WeaponView {
                    ty: t1.as_deref(),
                    counts_as_all_1h: a1,
                },
                weapon2: crate::calc_tools::WeaponView {
                    ty: t2.as_deref(),
                    counts_as_all_1h: a2,
                },
                main_socket_group: env.main_socket_group,
                affected_by_energy_blade,
            };
            crate::skills::setup_skills(&mut st, &inp).map_err(InitEnvError)?
        };
        prof.mark("(18c) setup_skills");
        env.requirements_table_gems = out.requirements_table_gems;
        env.unimplemented.extend(out.unimplemented);
        // (18c') CalcSetup.lua:1941-1945 buildActiveSkillModList for every active skill (R6.4)
        let more = {
            let p = player.borrow();
            let cfg_or = |k: &str| -> pob_mods::Value {
                let conv = |v: &pob_build::ConfigValue| match v {
                    pob_build::ConfigValue::Num(x) => pob_mods::Value::Num(*x),
                    pob_build::ConfigValue::Str(t) => pob_mods::Value::str(t),
                    pob_build::ConfigValue::Bool(b) => pob_mods::Value::Bool(*b),
                };
                let input = env.config.input.get(k).map(conv).unwrap_or_default();
                if input.truthy() {
                    input
                } else {
                    env.config.placeholder.get(k).map(conv).unwrap_or_default()
                }
            };
            // `build.spectreList`: Build.lua:989-992 keeps only ids with `data.minions[id]`
            let spectres: Vec<String> = build
                .spectres
                .iter()
                .filter(|id| crate::skill_modlist::is_minion(id))
                .cloned()
                .collect();
            let inp = crate::skill_modlist::ModListInputs {
                mod_db: &mod_db,
                enemy_db: &env.enemy_db,
                item_list: &p.item_list,
                weapon_data1: &p.weapon_data1,
                weapon_data2: &p.weapon_data2,
                mode_buffs: env.mode_buffs,
                mode_combat: env.mode_combat,
                mode_effective: env.mode_effective,
                melee_distance: cfg_or("meleeDistance"),
                projectile_distance: cfg_or("projectileDistance"),
                spectre_list: &spectres,
                character_level: build.level,
                enemy_level: env.enemy_level,
            };
            crate::skill_modlist::build_all(&mut st, &inp).map_err(InitEnvError)?
        };
        prof.mark("(18c') buildActiveSkillModList");
        env.unimplemented.extend(more);
        env.skills = Some(st);
    }

    // (18d) the main skill perform reads: with the skill phase, `env.skills` (R6.5); without it, the CALC-ARCH §8 stub.
    {
        let mut p = player.borrow_mut();
        p.main_skill = if env.skills.is_some() {
            None
        } else {
            Some(MainSkillStub {
                skill_mod_list: ModStore::new_list(None),
                granted_effect_name: "Melee",
            })
        };
        p.active_skill_list = Vec::new();
    }

    // --- (19) requirements (:1972); the gem half is P5 -----------------------------------------------------------------
    env.requirements_table = env
        .requirements_table_items
        .iter()
        .chain(env.requirements_table_gems.iter())
        .cloned()
        .collect();
    if env.skills.is_none() {
        env.unimplemented
            .push("CalcSetup.lua:1972 requirementsTableGems (gem requirements need data.gems, P5)".into());
    }
    prof.mark("(18d)-(19) rest");

    Ok(env)
}
