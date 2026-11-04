//! The state `Classes/ItemsTab.lua` mutates: `self.items`, `self.itemOrderList`, `self.itemSets`, `self.itemSetOrderList`, `slot.selItemId`, `spec.jewels`.
//! Every mutator writes the model (what `SaveDB` + a load would read) and the run's live tables (`env.items`, `env.ordered_slots`: the tab draws from them before
//! the next frame's `buildFlag` run). The controls and `IsShown` live in `pob_ui::tabs::items`. Gate: `tests/items_tab.rs` (path equivalence).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_calc::env::CalcOverride;
use pob_calc::pob_build::{self, ItemModel, ItemSetModel, ItemSetSlotModel, ModRangeModel, NumMap};
use pob_items::Item;

use crate::engine::{Engine, active_spec_mut};

/// `ItemsTab:CreateUndoState` (:5081-5096): the items as their saved form, the slots through their sets.
#[derive(Clone)]
pub struct ItemsUndoState {
    pub active_item_set: f64,
    pub items: Vec<ItemModel>,
    pub item_order: Vec<f64>,
    pub item_sets: Vec<ItemSetModel>,
    /// `state.items`, a new table filled in the `pairs` order of the old one (:5024-5027).
    pub items_table: pob_tree::lj_table::LjTable,
    /// `state.slotSelItemId` of the socket slots: `spec.jewels` of the active spec.
    pub jewels: NumMap<f64>,
}

/// What `ItemsTab:Save` writes for one item: the id, the variants, `BuildAndParseRaw`'s text, the `ModRange`s.
pub fn item_model_of(id: f64, item: &Item) -> ItemModel {
    let mut it = item.clone();
    it.build_and_parse_raw();
    ItemModel {
        id,
        variant: it.variant,
        variant_alt: it.variant_alt[0],
        variant_alt2: it.variant_alt[1],
        variant_alt3: it.variant_alt[2],
        variant_alt4: it.variant_alt[3],
        variant_alt5: it.variant_alt[4],
        mod_ranges: crate::save::mod_ranges(&it)
            .into_iter()
            .map(|(n, range)| ModRangeModel { id: n as f64, range })
            .collect(),
        raw: it.raw,
    }
}

impl Engine {
    /// Before an edit of the items: `self.miscCalculator` is made by every `BuildOutput` (CalcsTab.lua:471), so upstream's `calcBase` is of the state before the edit until the next frame's run;
    /// its `calcFunc` reads the live slots ([`Engine::misc_dirty`]). Here the base pass is made only if an edit comes before any tooltip of this run.
    pub fn touch_misc(&self) {
        if self.misc.borrow().is_none() {
            let _ = self.misc_calculator();
        }
        self.misc_dirty.set(true);
    }

    /// `wipeGlobalCache()` of another build (CompareEntry.lua:183, :251, Build.lua:1260): the cache is one for all of them, the `calcFunc` this build already has runs over an empty one next.
    pub fn wipe_misc_cache(&self) {
        if let Ok(calc) = self.misc_calculator() {
            calc.session().wipe_global_cache();
        }
    }

    /// `self.items`.
    pub fn items(&self) -> &IndexMap<u64, Rc<Item>> {
        &self.result().env.items
    }

    /// `pairs(self.items)` (ItemSlotControl.lua:96): the order of the table's history, not of the ids or the file.
    pub fn items_pairs(&self) -> Vec<(u64, Rc<Item>)> {
        let items = &self.result().env.items;
        self.with_items_table(|t| {
            t.pairs_keys()
                .iter()
                .filter_map(|k| items.get(&(*k as u64)).map(|i| (*k as u64, i.clone())))
                .collect()
        })
    }

    fn with_items_table<R>(&self, f: impl FnOnce(&mut pob_tree::lj_table::LjTable) -> R) -> R {
        let mut t = self.items_table.borrow_mut();
        // `ItemsTab:Load` (:1267): `self.items[item.id] = item` in the order of the file
        let t = t.get_or_insert_with(|| {
            let mut t = pob_tree::lj_table::LjTable::new();
            for i in &self.model().items {
                t.set(i.id);
            }
            t
        });
        f(t)
    }

    /// `self.items[id] = item`.
    pub(crate) fn items_table_set(&self, id: f64) {
        self.with_items_table(|t| t.set(id));
    }

    /// `self.items[id] = nil`.
    fn items_table_remove(&self, id: f64) {
        self.with_items_table(|t| t.remove(id, pob_tree::lj_table::NIL_STORE_CREATES_KEY));
    }

    /// `self.items[id]`.
    pub fn item_by_id(&self, id: f64) -> Option<Rc<Item>> {
        self.result().env.item(id).cloned()
    }

    /// `self.itemOrderList`.
    pub fn item_order(&self) -> Vec<f64> {
        self.model().item_order.clone()
    }

    /// `self.itemSetOrderList` with the sets.
    pub fn item_sets(&self) -> &[ItemSetModel] {
        &self.model().item_sets
    }

    /// `self.activeItemSetId`.
    pub fn active_item_set_id(&self) -> f64 {
        pob_build::active_item_set(self.model()).id
    }

    /// `self.activeItemSet`.
    pub fn active_item_set(&self) -> &ItemSetModel {
        pob_build::active_item_set(self.model())
    }

    /// `slot.selItemId` by name.
    pub fn slot_sel_item(&self, slot_name: &str) -> f64 {
        self.result()
            .env
            .ordered_slots
            .iter()
            .find(|s| s.slot_name == slot_name)
            .map_or(0.0, |s| s.sel_item_id)
    }

    /// `self.showStatDifferences`.
    pub fn items_show_stat_differences(&self) -> bool {
        self.extras().items_show_stat_differences
    }

    /// Ctrl+D (:1541-1543).
    pub fn set_items_show_stat_differences(&mut self, on: bool) {
        self.extras_mut().items_show_stat_differences = on;
    }

    /// `ItemsTab:IsItemValidForSlot(item, slotName)` (:2617-2663) over the active set.
    pub fn is_item_valid_for_slot(&self, item: &Item, slot_name: &str) -> bool {
        self.is_item_valid_in_set(item, slot_name, self.active_item_set_id())
    }

    /// `ItemsTab:IsItemValidForSlot(item, slotName, itemSet)`.
    pub fn is_item_valid_in_set(&self, item: &Item, slot_name: &str, set_id: f64) -> bool {
        let env = &self.result().env;
        let active = set_id == self.active_item_set_id();
        let set = self.model().item_sets.iter().find(|s| s.id == set_id);
        pob_calc::init_env::is_item_valid_for_slot(item, slot_name, &env.spec, &env.items, |name| {
            if active {
                self.slot_sel_item(name)
            } else {
                set.and_then(|s| s.slots.get(name)).map_or(0.0, |e| e.item_id)
            }
        })
    }

    /// The write of `self.itemOrderList`: `<Item>`s are saved in this order, and `env.items` walks it.
    fn sync_item_order(&mut self) {
        let order = self.model().item_order.clone();
        let m = self.model_mut();
        m.items
            .sort_by_key(|im| order.iter().position(|o| *o == im.id).unwrap_or(usize::MAX));
        let res = self.res_mut();
        let mut new = IndexMap::with_capacity(res.env.items.len());
        for id in &order {
            if let Some(it) = res.env.items.get(&(*id as u64)) {
                new.insert(*id as u64, it.clone());
            }
        }
        res.env.items = new;
    }

    /// `ItemsTab:AddItem(item, true, index)` (:1739-1777) without the auto equip (the tab asks `slot:IsShown`): a new id when `item.id` is nil, else the item is replaced.
    pub fn add_item(&mut self, item: &Item, index: Option<usize>) -> Result<f64, String> {
        if item.base.is_none() {
            return Err("ItemsTab:AddItem: an item without a base".into());
        }
        let id = match item.id {
            Some(id) if self.item_by_id(id).is_some() => id,
            _ => {
                // :1741-1745 the first id not in `self.items`; the undo state keeps the id of a removed item
                let mut id = item.id.unwrap_or(1.0);
                while self.item_by_id(id).is_some() {
                    id += 1.0;
                }
                let m = self.model_mut();
                match index {
                    Some(i) => m.item_order.insert((i - 1).min(m.item_order.len()), id),
                    None => m.item_order.push(id),
                }
                id
            }
        };
        let model = item_model_of(id, item);
        let replacing = self.item_by_id(id);
        let cluster = replacing
            .as_ref()
            .is_some_and(|r| r.cluster_jewel.is_some() || item.cluster_jewel.is_some() || r.base_name.as_deref() == Some("Timeless Jewel"));
        let mut parsed = Item::parse(
            &model.raw,
            &pob_items::ItemOptions {
                parse_mode: pob_calc::ParseMode::Full,
                id: Some(id),
                variant: model.variant,
                variant_alt: [
                    model.variant_alt,
                    model.variant_alt2,
                    model.variant_alt3,
                    model.variant_alt4,
                    model.variant_alt5,
                ],
                mod_ranges: model.mod_ranges.iter().map(|r| (r.id, r.range)).collect(),
            },
        );
        parsed.id = Some(id);
        self.items_table_set(id);
        let m = self.model_mut();
        match m.items.iter_mut().find(|x| x.id == id) {
            Some(x) => *x = model,
            None => m.items.push(model),
        }
        self.res_mut().env.items.insert(id as u64, Rc::new(parsed));
        self.sync_item_order();
        if cluster && self.result().env.spec.jewels.0.iter().any(|(_, i)| *i == id) {
            self.rebuild_cluster_graphs()?;
        }
        Ok(id)
    }

    /// `build.spec:BuildClusterJewelGraphs()`.
    fn rebuild_cluster_graphs(&mut self) -> Result<(), String> {
        let mut spec = (*self.result().env.spec).clone();
        let items = self.result().env.items.clone();
        let lookup = move |id: f64| crate::tree_tab::cluster_lookup(&items, id);
        spec.build_cluster_jewel_graphs(&lookup).map_err(|e| e.0)?;
        self.write_spec_to_model(spec);
        Ok(())
    }

    /// `slot:SetSelItemId(id)` of a gear slot (ItemSlotControl.lua:81-83): `activeItemSet[slotName].selItemId`.
    pub fn set_slot_sel_item(&mut self, slot_name: &str, id: f64) {
        let active = self.active_item_set_id();
        let set = self
            .model_mut()
            .item_sets
            .iter_mut()
            .find(|s| s.id == active)
            .expect("active item set");
        match set.slots.get_mut(slot_name) {
            Some(e) => e.item_id = id,
            None => {
                set.slots.insert(
                    slot_name.to_owned(),
                    ItemSetSlotModel {
                        item_id: id,
                        active: false,
                        pb_url: None,
                    },
                );
            }
        }
        if let Some(s) = self.res_mut().env.ordered_slots.iter_mut().find(|s| s.slot_name == slot_name) {
            s.sel_item_id = id;
        }
    }

    /// `slot:SetSelItemId(id)` of a socket (ItemSlotControl.lua:73-79): `spec.jewels[nodeId]`, the graphs when it changed.
    pub fn set_socket_item(&mut self, node_id: u32, id: f64) -> Result<(), String> {
        let n = f64::from(node_id);
        let old = self.result().env.spec.jewels.get(n).copied().unwrap_or(0.0);
        let sm = active_spec_mut(self.model_mut());
        sm.jewels.0.retain(|(k, _)| *k != n);
        if id > 0.0 {
            sm.jewels.set(n, id);
        }
        let name = format!("Jewel {node_id}");
        if let Some(s) = self.res_mut().env.ordered_slots.iter_mut().find(|s| s.slot_name == name) {
            s.sel_item_id = id;
        }
        // `Populate` writes 0 to every empty socket on each call: the spec already says so
        if self.result().env.spec.jewels.get(n).copied() == Some(id) {
            return Ok(());
        }
        let mut spec = (*self.result().env.spec).clone();
        spec.jewels.set(n, id);
        if id == old {
            self.res_mut().env.spec = Rc::new(spec);
            return Ok(());
        }
        let items = self.result().env.items.clone();
        let lookup = move |id: f64| crate::tree_tab::cluster_lookup(&items, id);
        spec.build_cluster_jewel_graphs(&lookup).map_err(|e| e.0)?;
        self.write_spec_to_model(spec);
        Ok(())
    }

    /// The flask checkbox (ItemSlotControl.lua:43-48).
    pub fn set_slot_active(&mut self, slot_name: &str, state: bool) {
        let active = self.active_item_set_id();
        let set = self
            .model_mut()
            .item_sets
            .iter_mut()
            .find(|s| s.id == active)
            .expect("active item set");
        let e = set.slots.entry(slot_name.to_owned()).or_insert(ItemSetSlotModel {
            item_id: 0.0,
            active: false,
            pb_url: None,
        });
        e.active = state;
        if let Some(s) = self.res_mut().env.ordered_slots.iter_mut().find(|s| s.slot_name == slot_name) {
            s.active = Some(state);
        }
    }

    /// `activeItemSet.useSecondWeaponSet = state` + the main group of :241-249 / :261-269.
    pub fn set_use_second_weapon_set(&mut self, state: bool) {
        let active = self.active_item_set_id();
        self.extras_mut().nil_second_weapon.retain(|x| *x != active);
        self.model_mut()
            .item_sets
            .iter_mut()
            .find(|s| s.id == active)
            .expect("active item set")
            .use_second_weapon_set = state;
        let weapon_set = |e: &Engine, slot: &str| {
            e.result()
                .env
                .ordered_slots
                .iter()
                .find(|s| s.slot_name == slot)
                .and_then(|s| s.weapon_set)
        };
        let leaving = if state { 1.0 } else { 2.0 };
        let entering = if state { 2.0 } else { 1.0 };
        let target = {
            let groups = self.socket_groups();
            let main = self.main_socket_group() as usize;
            main.checked_sub(1)
                .and_then(|i| groups.get(i))
                .and_then(|g| g.slot.as_deref())
                .filter(|slot| weapon_set(self, slot) == Some(leaving))
                .and_then(|_| {
                    groups
                        .iter()
                        .position(|x| x.slot.as_deref().is_some_and(|s| weapon_set(self, s) == Some(entering)))
                })
        };
        if let Some(i) = target {
            self.set_main_group(i + 1);
        }
    }

    /// `ItemsTab:DeleteItem(item)` (:1910-1957) without its `PopulateSlots`: the slots, every set, the order, the sockets of every spec.
    pub fn delete_item(&mut self, id: f64) -> Result<(), String> {
        self.items_table_remove(id);
        for s in &mut self.res_mut().env.ordered_slots {
            if s.sel_item_id == id {
                s.sel_item_id = 0.0;
            }
        }
        let m = self.model_mut();
        for set in &mut m.item_sets {
            for e in set.slots.values_mut() {
                if e.item_id == id {
                    e.item_id = 0.0;
                }
            }
        }
        m.item_order.retain(|x| *x != id);
        m.items.retain(|x| x.id != id);
        let active_spec: *const pob_build::SpecModel = pob_build::active_spec(m);
        let mut active_changed = false;
        for sm in &mut m.tree.specs {
            let mut hit = Vec::new();
            sm.jewels.0.retain(|(n, i)| {
                if *i == id {
                    hit.push(*n);
                    false
                } else {
                    true
                }
            });
            if !hit.is_empty() {
                // :1938-1945 `spec.nodes[nodeId].alloc = false`, `spec.allocNodes[nodeId] = nil`
                sm.nodes.retain(|n| !hit.contains(n));
                if std::ptr::eq(&*sm, active_spec) {
                    active_changed = true;
                }
            }
        }
        self.res_mut().env.items.shift_remove(&(id as u64));
        if active_changed {
            self.reset_live_spec()?;
        }
        Ok(())
    }

    /// `build.spec` from the model: what the next run reads, the tab needs it in this frame.
    fn reset_live_spec(&mut self) -> Result<(), String> {
        self.set_live_spec(None);
        let sm = pob_build::active_spec(self.model()).clone();
        let spec = self.spec_of_model(&sm)?;
        self.res_mut().env.spec = Rc::new(spec);
        Ok(())
    }

    fn spec_of_model(&self, sm: &pob_build::SpecModel) -> Result<pob_tree::PassiveSpec, String> {
        let items = &self.result().env.items;
        let jewels = pob_calc::init_env::spec_jewels_of(&sm.jewels, items).map_err(|e| e.0)?;
        let mut s = pob_tree::PassiveSpec::load_mode_jewels(sm, pob_calc::ParseMode::Full, jewels).map_err(|e| e.0)?;
        pob_calc::init_env::post_load_spec(&mut s, items).map_err(|e| e.0)?;
        Ok(s)
    }

    /// `Del All` (ItemListControl.lua:54-71): no item, no socket.
    pub fn delete_all_items(&mut self) -> Result<(), String> {
        for id in self.model().items.iter().map(|i| i.id).collect::<Vec<_>>() {
            self.items_table_remove(id);
        }
        for s in &mut self.res_mut().env.ordered_slots {
            s.sel_item_id = 0.0;
        }
        let m = self.model_mut();
        for set in &mut m.item_sets {
            for e in set.slots.values_mut() {
                e.item_id = 0.0;
            }
        }
        m.items.clear();
        m.item_order.clear();
        for sm in &mut m.tree.specs {
            sm.jewels.0.clear();
        }
        self.res_mut().env.items.clear();
        self.reset_live_spec()
    }

    /// `self.itemOrderList` after a drag in the list (`ListControl` moved the row) or `SortItemList`.
    pub fn set_item_order(&mut self, order: Vec<f64>) {
        self.model_mut().item_order = order;
        self.sync_item_order();
    }

    /// `ItemsTab:NewItemSet()` (:1626-1641) + the order: the set of the Manage popup's `Save`.
    pub fn new_item_set(&mut self, title: Option<&str>, index: Option<usize>) -> f64 {
        let m = self.model_mut();
        let mut id = 1.0;
        while m.item_sets.iter().any(|s| s.id == id) {
            id += 1.0;
        }
        let set = ItemSetModel {
            id,
            title: title.map(str::to_owned),
            use_second_weapon_set: false,
            slots: IndexMap::new(),
            socket_id_urls: NumMap::default(),
        };
        match index {
            Some(i) => m.item_sets.insert((i - 1).min(m.item_sets.len()), set),
            None => m.item_sets.push(set),
        }
        self.extras_mut().nil_second_weapon.push(id);
        id
    }

    /// `copyTable(itemSets[id])` + `RenameSet(newSet, true)`'s `Save` (ItemSetListControl.lua:17-25, :54-61).
    pub fn copy_item_set(&mut self, from: f64, title: &str) -> f64 {
        let m = self.model_mut();
        let mut id = 1.0;
        while m.item_sets.iter().any(|s| s.id == id) {
            id += 1.0;
        }
        let mut set = m.item_sets.iter().find(|s| s.id == from).expect("item set").clone();
        set.id = id;
        set.title = Some(title.to_owned());
        m.item_sets.push(set);
        // `copyTable`: a nil stays nil
        if self.extras().nil_second_weapon.contains(&from) {
            self.extras_mut().nil_second_weapon.push(id);
        }
        id
    }

    pub fn rename_item_set(&mut self, id: f64, title: &str) {
        if let Some(s) = self.model_mut().item_sets.iter_mut().find(|s| s.id == id) {
            s.title = Some(title.to_owned());
        }
    }

    /// `set[slotName].selItemId` of any set (`ReceiveDrag` of a shared set; `EquipItemInSet`).
    pub fn set_item_set_slot(&mut self, set_id: f64, slot_name: &str, id: f64) {
        if set_id == self.active_item_set_id() {
            return self.set_slot_sel_item(slot_name, id);
        }
        let set = self.model_mut().item_sets.iter_mut().find(|s| s.id == set_id).expect("item set");
        match set.slots.get_mut(slot_name) {
            Some(e) => e.item_id = id,
            None => {
                set.slots.insert(
                    slot_name.to_owned(),
                    ItemSetSlotModel {
                        item_id: id,
                        active: false,
                        pb_url: None,
                    },
                );
            }
        }
    }

    /// `t_remove(list, index)`, `itemSets[id] = nil`; the next active set is the caller's (ItemSetListControl.lua:127-133).
    pub fn delete_item_set(&mut self, id: f64) {
        self.model_mut().item_sets.retain(|s| s.id != id);
    }

    /// The list's drag (ItemSetListControl.lua:112-114).
    pub fn set_item_set_order(&mut self, order: &[f64]) {
        let m = self.model_mut();
        m.item_sets
            .sort_by_key(|s| order.iter().position(|o| *o == s.id).unwrap_or(usize::MAX));
    }

    /// `ItemsTab:SetActiveItemSet(id)` (:1644-1670) without `PopulateSlots`/`SyncLoadouts`: a gone id is the first of the list. Returns the id.
    pub fn set_active_item_set(&mut self, id: f64) -> f64 {
        let m = self.model_mut();
        let id = if m.item_sets.iter().any(|s| s.id == id) {
            id
        } else {
            m.item_sets[0].id
        };
        m.active_item_set = id;
        self.reload_gear_slots();
        id
    }

    /// `slot.selItemId = curSet[slotName].selItemId`, `slot.active = ...`.
    fn reload_gear_slots(&mut self) {
        let set = self.active_item_set().clone();
        for s in &mut self.res_mut().env.ordered_slots {
            if s.node_id.is_none() {
                let e = set.slots.get(&s.slot_name);
                s.sel_item_id = e.map_or(0.0, |e| e.item_id);
                s.active = e.map(|e| e.active);
            }
        }
    }

    /// `ItemsTab:CreateUndoState`.
    pub fn items_undo_state(&self) -> ItemsUndoState {
        let m = self.model();
        ItemsUndoState {
            active_item_set: m.active_item_set,
            items: m.items.clone(),
            item_order: m.item_order.clone(),
            item_sets: m.item_sets.clone(),
            items_table: self.with_items_table(|t| {
                let mut copy = pob_tree::lj_table::LjTable::new();
                for k in t.pairs_keys() {
                    copy.set(k);
                }
                copy
            }),
            jewels: pob_build::active_spec(m).jewels.clone(),
        }
    }

    /// The state `ResetUndo` keeps at the end of `ItemsTab:Load` (:1378): the sockets are 0 then, the tree comes later (`PopulateSlots` of TreeTab:PostLoad).
    pub fn items_load_undo_state(&self) -> ItemsUndoState {
        let mut st = self.items_undo_state();
        st.jewels = NumMap::default();
        st
    }

    /// `ItemsTab:RestoreUndoState` (:5098-5115) without `PopulateSlots`.
    pub fn restore_items_undo_state(&mut self, st: &ItemsUndoState) -> Result<(), String> {
        let changed = pob_build::active_spec(self.model()).jewels.0 != st.jewels.0;
        *self.items_table.borrow_mut() = Some(st.items_table.clone());
        let m = self.model_mut();
        m.active_item_set = st.active_item_set;
        m.items.clone_from(&st.items);
        m.item_order.clone_from(&st.item_order);
        m.item_sets.clone_from(&st.item_sets);
        active_spec_mut(m).jewels = st.jewels.clone();
        let items = pob_calc::init_env::parse_items(self.model(), pob_calc::ParseMode::Full);
        self.res_mut().env.items = items;
        self.reload_gear_slots();
        if changed {
            let jewels = st.jewels.clone();
            for s in &mut self.res_mut().env.ordered_slots {
                if let Some(n) = s.node_id {
                    s.sel_item_id = jewels.get(f64::from(n)).copied().unwrap_or(0.0);
                }
            }
            // :5104-5106 `SetSelItemId` of every socket: the graphs are rebuilt on the live spec, the allocation stays as it is then (a subgraph's nodes do not come back with its jewel)
            let mut spec = (*self.result().env.spec).clone();
            spec.jewels = jewels;
            let items = self.result().env.items.clone();
            let lookup = move |id: f64| crate::tree_tab::cluster_lookup(&items, id);
            spec.build_cluster_jewel_graphs(&lookup).map_err(|e| e.0)?;
            self.write_spec_to_model(spec);
        }
        Ok(())
    }

    /// `self.calcsTab.mainOutput[key]`.
    pub fn output_value(&self, key: &str) -> Option<f64> {
        match self.result().stats.get(key) {
            Some(pob_calc::StatValue::Num(n)) => Some(*n),
            _ => None,
        }
    }

    /// `self.build.configTab.input[name]` is truthy.
    pub fn config_flag_set(&self, name: &str) -> bool {
        matches!(self.result().env.config.input.get(name), Some(pob_build::InputValue::Bool(true)))
    }

    /// `data.powerStatList.GetFromOutput(calcFunc(override, useFullDPS), stat)` (ItemDBControl.lua:263-264).
    pub fn item_override_power(&self, ov: CalcOverride, stat: &pob_calc::power::PowerStat) -> Result<f64, String> {
        let calc = self.misc_calculator()?;
        let run = calc
            .calc(
                &Rc::new(ov),
                (stat.stat.as_deref() == Some("FullDPS")).then_some(true),
                self.view_mode_is_tree.get(),
            )
            .map_err(|e| e.0)?;
        let r = run.borrow();
        Ok(pob_calc::power::get_from_output(&r.state.player.output, stat))
    }

    /// `buildSpecForJewelComparison(itemsTab, compareSlot, replacementItem)` (ItemsTab.lua:4216-4243): a spec with the jewel in the socket, its depends and paths built anew.
    pub fn spec_for_jewel_comparison(&self, node_id: u32, item: Option<Rc<Item>>) -> Result<Rc<pob_tree::PassiveSpec>, String> {
        let mut sm = pob_build::active_spec(self.model()).clone();
        let mut items = self.result().env.items.clone();
        let n = f64::from(node_id);
        sm.jewels.0.retain(|(k, _)| *k != n);
        if let Some(it) = item {
            let id = it
                .id
                .filter(|i| self.item_by_id(*i).is_some_and(|x| Rc::ptr_eq(&x, &it)))
                .unwrap_or_else(|| {
                    let mut t = 1.0;
                    while items.contains_key(&(t as u64)) {
                        t += 1.0;
                    }
                    items.insert(t as u64, it);
                    // :4167 and :4177
                    self.items_table_set(t);
                    self.items_table_remove(t);
                    t
                });
            sm.jewels.set(n, id);
        }
        let jewels = pob_calc::init_env::spec_jewels_of(&sm.jewels, &items).map_err(|e| e.0)?;
        let mut s = pob_tree::PassiveSpec::load_mode_jewels(&sm, pob_calc::ParseMode::Full, jewels).map_err(|e| e.0)?;
        pob_calc::init_env::post_load_spec(&mut s, &items).map_err(|e| e.0)?;
        Ok(Rc::new(s))
    }

    /// `calcFunc(override)`.
    pub fn item_override_run(&self, ov: CalcOverride) -> Result<pob_calc::session::RunRef, String> {
        let calc = self.misc_calculator()?;
        calc.calc(&Rc::new(ov), None, self.view_mode_is_tree.get()).map_err(|e| e.0)
    }

    /// `AddStatComparesToTooltip(tooltip, calcBase, output, header)` over a run of [`Engine::item_override_run`].
    pub fn item_compare_run(&self, run: &pob_calc::session::RunRef, header: &str) -> Result<Vec<String>, String> {
        let calc = self.misc_calculator()?;
        self.stat_compare_lines(&calc.base, run, header)
    }

    /// `calcFunc(override)` + `AddStatComparesToTooltip(tooltip, calcBase, output, header)` as lines.
    pub fn item_compare_lines(&self, ov: CalcOverride, header: &str) -> Result<Vec<String>, String> {
        self.compare_override(ov, header)
    }

    /// `AddStatComparesToTooltip(tooltip, calcFunc(a), calcFunc(b), header)` (:2579-2581, :3117-3119): both sides are overrides.
    pub fn item_compare_two(&self, a: CalcOverride, b: CalcOverride, header: &str) -> Result<Vec<String>, String> {
        let calc = self.misc_calculator()?;
        let tree = self.view_mode_is_tree.get();
        let base = calc.calc(&Rc::new(a), None, tree).map_err(|e| e.0)?;
        let run = calc.calc(&Rc::new(b), None, tree).map_err(|e| e.0)?;
        self.stat_compare_lines(&base, &run, header)
    }
}
