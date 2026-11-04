//! `Classes/ItemsTab.lua` (the tab, its slots and the panel of the display item), `ItemSlotControl.lua`, `ItemListControl.lua`, `ItemSetListControl.lua`, `SharedItemListControl.lua`, `SharedItemSetListControl.lua`, `ItemDBControl.lua`.
//! The items live in the engine (`pob_engine::items_tab`); the display item is an owned `Rc<Item>` here (upstream's `self.displayItem`: not in `self.items` even when it has the id of one).
//! The popups of the display item: `item_editor.rs`. Not ported (a note, no crash): `Trade for these items` and `Buy similar` (the network, U8), `buildSpecForJewelComparison`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_engine::engine::Engine;
use pob_engine::items_tab::ItemsUndoState;
use pob_gfx::{Frame, Gfx, ImageHandle};
use pob_items::Item;

use super::gem_select::gem_of;
use crate::colors::color_code;
use crate::control::{Control, ControlBase, ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{DragTarget, ListControl, ListHooks, SelfTarget};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::item_tooltip::{CompareSlot, ItemTooltipArgs, add_item_tooltip};
use crate::modules::build::BuildMode;
use crate::modules::main::{BareControl, main, note_not_ported};
use crate::sg;
use crate::tooltip::Tooltip;
use crate::undo::{UndoHandler, UndoHost};

pub(crate) fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

/// `baseSlots` (:37).
pub const BASE_SLOTS: [&str; 18] = [
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

/// A row of the lists an item can be dragged between: an item of the build by id, or an item object (database, shared list).
#[derive(Clone)]
pub enum ItemRow {
    Id(f64),
    Obj(Rc<Item>),
}

impl PartialEq for ItemRow {
    fn eq(&self, o: &Self) -> bool {
        match (self, o) {
            (Self::Id(a), Self::Id(b)) => a == b,
            (Self::Obj(a), Self::Obj(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------- ItemSlotControl

pub struct ItemSlotControl {
    pub drop: Rc<DropDownControl>,
    pub tab: Weak<ItemsTab>,
    pub slot_name: String,
    pub slot_num: Option<f64>,
    pub label: RefCell<String>,
    pub label_offset: f64,
    pub node_id: Option<u32>,
    pub weapon_set: Cell<Option<f64>>,
    pub abyssal_socket_list: RefCell<Vec<Rc<ItemSlotControl>>>,
    pub inactive: Cell<bool>,
    /// `self.items`: the ids behind `self.list`.
    pub items: RefCell<Vec<f64>>,
    pub sel_item_id: Cell<f64>,
    pub activate: RefCell<Option<Rc<CheckBoxControl>>>,
    pub active: Cell<Option<bool>>,
}

impl ItemSlotControl {
    /// `ItemSlotClass:ItemSlotControl(anchor, x, y, itemsTab, slotName, slotLabel, nodeId)`.
    fn new(
        g: &mut Gfx,
        prev: &ControlRef,
        tab: Weak<ItemsTab>,
        slot_name: String,
        slot_label: Option<&str>,
        node_id: Option<u32>,
    ) -> Rc<Self> {
        let slot_num = slot_digits(&slot_name);
        let t = tab.clone();
        let name = slot_name.clone();
        let drop = DropDownControl::new(
            g,
            anchor("TOPLEFT", prev, "BOTTOMLEFT"),
            rect(0.0, 2.0, 310.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t.upgrade().expect("itemsTab");
                let c = s.slot(&name);
                let id = c.items.borrow()[index - 1];
                if id != c.sel_item_id.get() {
                    c.set_sel_item_id(id);
                    s.populate_slots(g);
                    s.add_undo_state(false);
                    s.build().build_flag.set(true);
                }
            })),
            None,
            false,
        );
        drop.base.anchor.borrow_mut().collapse = true;
        let label_offset = if slot_name.contains("Flask") { -24.0 } else { -2.0 };
        let c = Rc::new_cyclic(|me: &Weak<ItemSlotControl>| {
            let m = me.clone();
            drop.base
                .enabled
                .set_fn(move |_| m.upgrade().expect("slot").items.borrow().len() > 1);
            let m = me.clone();
            drop.base.shown.set_fn(move |_| !m.upgrade().expect("slot").inactive.get());
            let m = me.clone();
            drop.tip.set_tooltip_func(move |tooltip, g, (mode, index, _value)| {
                let s = m.upgrade().expect("slot");
                let tab = s.tab.upgrade().expect("itemsTab");
                let id = s.items.borrow().get(index.wrapping_sub(1)).copied().unwrap_or(0.0);
                let item = tab.read(|e| e.item_by_id(id));
                let sel = tab.host.sel();
                let activate = s.activate.borrow().clone();
                let blocked = !s.drop.dropped.get()
                    && sel
                        .as_ref()
                        .is_some_and(|c| !activate.as_ref().is_some_and(|a| crate::control::same(&**a, &**c)) && !tab.is_list_control(c));
                if !main().popups.borrow().is_empty() || mode == "OUT" || item.is_none() || blocked {
                    tooltip.clear(true);
                } else if let Some(item) = item {
                    let key = (
                        Rc::as_ptr(&item) as usize,
                        tab.read(Engine::output_revision),
                        sg::is_key_down(g, "SHIFT"),
                    );
                    if tooltip.check_for_update(key) {
                        tab.add_tooltip(tooltip, g, &item, Some(&s.compare_info()), false);
                    }
                }
            });
            ItemSlotControl {
                drop: drop.clone(),
                tab,
                slot_num,
                label: RefCell::new(slot_label.unwrap_or(&slot_name).to_string()),
                label_offset,
                node_id,
                weapon_set: Cell::new(None),
                abyssal_socket_list: RefCell::new(Vec::new()),
                inactive: Cell::new(false),
                items: RefCell::new(Vec::new()),
                sel_item_id: Cell::new(0.0),
                activate: RefCell::new(None),
                active: Cell::new(None),
                slot_name,
            }
        });
        if c.slot_name.contains("Flask") {
            let r: ControlRef = c.clone();
            let w = Rc::downgrade(&c);
            let cb = CheckBoxControl::new(
                g,
                anchor("RIGHT", &r, "LEFT"),
                [-2.0, 0.0, 20.0],
                None,
                Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                    let s = w.upgrade().expect("slot");
                    s.active.set(Some(state));
                    let t = s.tab.upgrade().expect("itemsTab");
                    t.edit(|e| {
                        e.set_slot_active(&s.slot_name, state);
                        Ok(())
                    });
                    t.add_undo_state(false);
                    t.build().build_flag.set(true);
                })),
                Some("Activate this flask."),
                false,
            );
            let w = Rc::downgrade(&c);
            cb.base.enabled.set_fn(move |_| w.upgrade().expect("slot").sel_item_id.get() != 0.0);
            c.drop.host.add("activate", cb.clone());
            *c.activate.borrow_mut() = Some(cb);
        }
        c
    }

    fn compare_info(&self) -> CompareSlot {
        CompareSlot {
            slot_name: self.slot_name.clone(),
            label: self.label.borrow().clone(),
            sel_item_id: self.sel_item_id.get(),
            node_id: self.node_id,
            slot_num: self.slot_num,
        }
    }

    /// `ItemSlotClass:SetSelItemId(selItemId)`.
    pub fn set_sel_item_id(&self, id: f64) {
        let t = self.tab.upgrade().expect("itemsTab");
        // the same id changes nothing the calculator reads (`Populate` writes 0 to every empty socket on each call)
        if self.sel_item_id.get() != id {
            t.read(Engine::touch_misc);
        }
        if let Some(n) = self.node_id {
            t.edit(|e| e.set_socket_item(n, id));
        } else {
            t.edit(|e| {
                e.set_slot_sel_item(&self.slot_name, id);
                Ok(())
            });
        }
        self.sel_item_id.set(id);
    }

    /// `ItemSlotClass:Populate()`.
    pub fn populate(&self, g: &Gfx) {
        let t = self.tab.upgrade().expect("itemsTab");
        if let Some(n) = self.node_id {
            let id = t.read(|e| e.spec_rc().jewels.get(f64::from(n)).copied().unwrap_or(0.0));
            self.sel_item_id.set(id);
        }
        let mut ids = vec![0.0];
        let mut list = vec![DropItem::str("None")];
        let mut sel = 1;
        let cur = self.sel_item_id.get();
        t.read(|e| {
            for (id, item) in &e.items_pairs() {
                if e.is_item_valid_for_slot(item, &self.slot_name) {
                    ids.push(*id as f64);
                    list.push(DropItem::str(&format!("{}{}", cc(&item.rarity), item.name)));
                    if *id as f64 == cur {
                        sel = list.len();
                    }
                }
            }
        });
        *self.items.borrow_mut() = ids;
        *self.drop.list.borrow_mut() = list;
        self.drop.sel_index.set(sel);
        let _ = g;
        let valid = t.read(|e| e.item_by_id(cur).is_some_and(|i| e.is_item_valid_for_slot(&i, &self.slot_name)));
        if !valid {
            self.set_sel_item_id(0.0);
        }
        let mut count = 0.0;
        if self.sel_item_id.get() > 0.0 {
            count = t.read(|e| e.item_by_id(self.sel_item_id.get()).map_or(0.0, |i| i.abyssal_socket_count));
        }
        for (i, a) in self.abyssal_socket_list.borrow().iter().enumerate() {
            let inactive = (i + 1) as f64 > count;
            a.inactive.set(inactive);
            if inactive {
                a.set_sel_item_id(0.0);
            }
        }
    }

    /// `ItemSlotClass:ReceiveDrag(type, value, source)`.
    fn receive_item(&self, g: &mut Gfx, value: &ItemRow) {
        let t = self.tab.upgrade().expect("itemsTab");
        match value {
            ItemRow::Id(id) => self.set_sel_item_id(*id),
            ItemRow::Obj(item) => {
                let id = t.add_new_from_raw(g, &item.raw);
                self.set_sel_item_id(id);
            }
        }
        t.populate_slots(g);
        t.add_undo_state(false);
        t.build().build_flag.set(true);
    }
}

/// `tonumber(slotName:match("%d+$") or slotName:match("%d+"))`.
fn slot_digits(name: &str) -> Option<f64> {
    let tail: String = name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if !tail.is_empty() {
        return tail.parse().ok();
    }
    let first: String = name
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    first.parse().ok()
}

impl Control for ItemSlotControl {
    fn base(&self) -> &ControlBase {
        &self.drop.base
    }
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        self.drop.is_mouse_over(g)
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// ItemSlotControl.lua:144-161
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (_, height) = self.get_size(f);
        sg::draw_string(
            f,
            x + self.label_offset,
            y + 2.0,
            Some("RIGHT_X"),
            height - 4.0,
            "VAR",
            &format!("^7{}:", self.label.borrow()),
        );
        // `self.DropDownControl:Draw(viewPort)`: `noTooltip` stops here
        let _ = no_tooltip;
        self.drop.draw(f, view_port, false);
        self.drop.host.draw_controls(f, view_port, None);
        let t = self.tab.upgrade().expect("itemsTab");
        if main().popups.borrow().is_empty()
            && let Some(n) = self.node_id
            && (self.drop.dropped.get()
                || (self.is_mouse_over(f) == Some(true) && (self.drop.other_drag_source.get() || t.host.sel().is_none())))
        {
            let viewer_y = if self.drop.drop_up.get() && self.drop.dropped.get() {
                y + 20.0
            } else {
                f64::min(y - 280.0 - 4.0, view_port.y + view_port.height - 280.0)
            };
            t.draw_socket_viewer(f, n, x, viewer_y, 308.0, 280.0);
        }
    }
    fn has_on_char(&self) -> bool {
        true
    }
    fn on_char(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        self.drop.clone().on_char(g, key).map(|_| self as ControlRef)
    }
    fn has_on_key_down(&self) -> bool {
        true
    }
    /// ItemSlotControl.lua:163-172
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let (Some(m), Some(a)) = (self.drop.host.get_mouse_over_control(g), self.activate.borrow().clone())
            && crate::control::same(&*m, &*a)
        {
            return a.on_key_down(g, key, double_click);
        }
        self.drop.clone().on_key_down(g, key, double_click).map(|_| self as ControlRef)
    }
    fn has_on_key_up(&self) -> bool {
        true
    }
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        self.drop.clone().on_key_up(g, key).map(|_| self as ControlRef)
    }
    fn has_on_hover_key_up(&self) -> bool {
        true
    }
    /// ItemSlotControl.lua:174-185
    fn on_hover_key_up(&self, g: &mut Gfx, key: &str) {
        if key == "F1" {
            let index = self.drop.get_hover_index();
            let id = self.items.borrow().get((index - 1) as usize).copied().unwrap_or(0.0);
            let t = self.tab.upgrade().expect("itemsTab");
            if let Some(item) = t.read(|e| e.item_by_id(id)) {
                wiki_open_item(g, &item);
            }
        }
    }
}

/// `itemLib.wiki.openItem(item)` (ItemTools.lua:466-470).
pub fn wiki_open_item(g: &mut Gfx, item: &Item) {
    let name = if item.rarity == "UNIQUE" || item.rarity == "RELIC" {
        item.title.clone().unwrap_or_default()
    } else {
        item.base_name.clone().unwrap_or_default()
    };
    wiki_open(g, &name);
}

/// `itemLib.wiki.open(name)` (ItemTools.lua:471-476).
pub fn wiki_open(g: &mut Gfx, name: &str) {
    let route = name.replace(' ', "_");
    g.open_url(&format!("https://www.poewiki.net/wiki/{route}"));
    main().wiki_triggered.set(true);
}

/// A slot as a drag target (`ItemSlotClass:CanReceiveDrag`).
struct SlotTarget(Weak<ItemSlotControl>);

impl DragTarget<ItemRow> for SlotTarget {
    fn can_receive_drag(&self, drag_type: &str, value: &ItemRow) -> bool {
        let s = self.0.upgrade().expect("slot");
        let t = s.tab.upgrade().expect("itemsTab");
        drag_type == "Item"
            && t.read(|e| {
                let item = match value {
                    ItemRow::Id(id) => e.item_by_id(*id),
                    ItemRow::Obj(i) => Some(i.clone()),
                };
                item.is_some_and(|i| e.is_item_valid_for_slot(&i, &s.slot_name))
            })
    }
    fn set_other_drag_source(&self, on: bool) {
        self.0.upgrade().expect("slot").drop.other_drag_source.set(on);
    }
    fn other_drag_source(&self) -> bool {
        self.0.upgrade().expect("slot").drop.other_drag_source.get()
    }
    fn is_mouse_over(&self, g: &Gfx) -> bool {
        self.0.upgrade().expect("slot").is_mouse_over(g) == Some(true)
    }
    fn receive_drag(&self, g: &mut Gfx, _drag_type: &str, value: &ItemRow) {
        self.0.upgrade().expect("slot").receive_item(g, value);
    }
}

// ---------------------------------------------------------------------------------------------- the tab

pub struct ItemsTab {
    pub host: ControlHost,
    pub(crate) me: Weak<Self>,
    build: Weak<BuildMode>,
    pub undo: UndoHandler<Rc<ItemsUndoState>>,
    pub(crate) anchor: Rc<BareControl>,
    pub set_select: Rc<DropDownControl>,
    pub slots: RefCell<IndexMap<String, Rc<ItemSlotControl>>>,
    pub sockets: RefCell<IndexMap<u32, Rc<ItemSlotControl>>>,
    pub last_slot: RefCell<Option<Rc<ItemSlotControl>>>,
    pub spec_select: RefCell<Option<Rc<DropDownControl>>>,
    pub item_list: RefCell<Option<Rc<ListControl<ItemRow>>>>,
    pub loadout_filter: RefCell<Option<Rc<DropDownControl>>>,
    loadout_list_key: RefCell<Option<String>>,
    last_output_revision: Cell<Option<u64>>,
    pub shared_list: RefCell<Option<Rc<ListControl<ItemRow>>>>,
    pub select_db_label: RefCell<Option<Rc<LabelControl>>>,
    pub select_db: RefCell<Option<Rc<DropDownControl>>>,
    pub unique_db: RefCell<Option<Rc<ItemDb>>>,
    pub rare_db: RefCell<Option<Rc<ItemDb>>>,
    pub anchor_display_item: RefCell<Option<Rc<BareControl>>>,
    pub set_manage: RefCell<Option<Rc<ButtonControl>>>,
    pub craft_display_item: RefCell<Option<Rc<ButtonControl>>>,
    pub scroll_bar_h: RefCell<Option<Rc<ScrollBarControl>>>,
    pub scroll_bar_v: RefCell<Option<Rc<ScrollBarControl>>>,
    pub tooltip_anchor: RefCell<Option<Rc<BareControl>>>,
    pub display_item: RefCell<Option<Rc<Item>>>,
    pub display_tooltip: RefCell<Tooltip>,
    pub panel: RefCell<Option<Rc<super::item_editor::Panel>>>,
    snap_h: Cell<Option<&'static str>>,
    pub max_y: Cell<f64>,
    socket_viewer: RefCell<Option<crate::tree_view::PassiveTreeView>>,
    /// The manage popup's lists.
    pub set_list: RefCell<Weak<ListControl<f64>>>,
    pub shared_set_list: RefCell<Weak<ListControl<usize>>>,
    pub(crate) foulborn_icon: ImageHandle,
    /// `self.lastCraftRaritySel`, `lastCraftTypeSel`, `lastCraftBaseSel`.
    pub(crate) last_craft: Cell<(usize, usize, usize)>,
    pub(crate) enchant_slot: Cell<usize>,
    pub(crate) anoint_enchant_slot: Cell<usize>,
}

impl UndoHost for ItemsTab {
    type State = Rc<ItemsUndoState>;
    fn undo_handler(&self) -> &UndoHandler<Self::State> {
        &self.undo
    }
    fn create_undo_state(&self) -> Self::State {
        Rc::new(self.read(Engine::items_undo_state))
    }
    fn restore_undo_state(&self, _state: Self::State) {
        unreachable!("ItemsTab restores through undo_g / redo_g: its controls need the host");
    }
}

impl ItemsTab {
    pub fn build(&self) -> Rc<BuildMode> {
        self.build.upgrade().expect("buildMode")
    }

    pub fn read<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        let b = self.build();
        let eng = b.engine.borrow();
        f(eng.as_ref().expect("engine"))
    }

    pub fn edit<R>(&self, f: impl FnOnce(&mut Engine) -> Result<R, String>) -> R {
        let b = self.build();
        let mut eng = b.engine.borrow_mut();
        f(eng.as_mut().expect("engine")).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn slot(&self, name: &str) -> Rc<ItemSlotControl> {
        self.slots.borrow().get(name).unwrap_or_else(|| panic!("slot {name}")).clone()
    }

    fn is_list_control(&self, c: &ControlRef) -> bool {
        let is = |l: &RefCell<Option<Rc<ListControl<ItemRow>>>>| l.borrow().as_ref().is_some_and(|x| crate::control::same(&**x, &**c));
        is(&self.item_list)
            || is(&self.shared_list)
            || [&self.unique_db, &self.rare_db]
                .iter()
                .any(|d| d.borrow().as_ref().is_some_and(|x| crate::control::same(&*x.list, &**c)))
    }

    /// `self.items[id]`-less tooltip: `AddItemTooltip(tooltip, item, slot, dbMode)`.
    pub fn add_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, item: &Rc<Item>, slot: Option<&CompareSlot>, db_mode: bool) {
        self.add_tooltip_width(tooltip, g, item, slot, db_mode, None);
    }

    /// `AddItemTooltip(tooltip, item, slot, dbMode, maxWidth)`.
    pub fn add_tooltip_width(
        &self,
        tooltip: &mut Tooltip,
        g: &Gfx,
        item: &Rc<Item>,
        slot: Option<&CompareSlot>,
        db_mode: bool,
        max_width: Option<f64>,
    ) {
        // :4911: the branch of the items that are not a flask or a tincture
        if !item.base.as_ref().is_some_and(|b| b.flask.is_some() || b.tincture.is_some()) {
            self.update_sockets(g);
        }
        let compare_slots = self.compare_slots(g, item);
        let empty_flask_slots = self
            .slots
            .borrow()
            .iter()
            .filter(|(n, s)| n.starts_with("Flask") && s.sel_item_id.get() == 0.0)
            .count() as f64;
        let args = ItemTooltipArgs {
            slot,
            db_mode,
            max_width,
            compare_slots,
            in_items_tab: self.build().view_mode.borrow().as_str() == "ITEMS",
            empty_flask_slots,
        };
        self.read(|e| add_item_tooltip(tooltip, g, e, item, &args));
    }

    /// `AddItemTooltip(tooltip, jewel, socket)` of the tree view (PassiveTreeView.lua:1501).
    pub fn add_socket_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, item: &Rc<Item>, node_id: u32) {
        // :4911 `UpdateSockets` runs first in `AddItemTooltip`, the label of the slot is read after it ("Socket #n")
        self.update_sockets(g);
        let slot = self.sockets.borrow().get(&node_id).map(|s| s.compare_info());
        self.add_tooltip(tooltip, g, item, slot.as_ref(), false);
    }

    /// `compareSpec.build.itemsTab:AddItemTooltip(tooltip, cJewel, socket)` with the socket of the primary build (PassiveTreeView.lua:1493).
    pub fn add_foreign_socket_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, item: &Rc<Item>, socket_tab: &ItemsTab, node_id: u32) {
        let slot = socket_tab.sockets.borrow().get(&node_id).map(|s| s.compare_info());
        self.add_tooltip(tooltip, g, item, slot.as_ref(), false);
    }

    /// `AddItemTooltip(tooltip, ttItem, slot)` of the Skills tab (SkillsTab.lua:198).
    pub fn add_slot_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, item: &Rc<Item>, slot_name: &str) {
        let slot = self.slots.borrow().get(slot_name).map(|s| s.compare_info());
        self.add_tooltip(tooltip, g, item, slot.as_ref(), false);
    }

    /// `AddItemStatDifferences(tooltip, jewel, jewel.base, socket)` (TimelessJewelListControl.lua:330-331).
    pub fn add_stat_differences(&self, tooltip: &mut Tooltip, g: &Gfx, item: &Rc<Item>, node_id: Option<u32>) {
        self.update_sockets(g);
        let slot = node_id.and_then(|n| self.sockets.borrow().get(&n).map(|s| s.compare_info()));
        let args = ItemTooltipArgs {
            slot: slot.as_ref(),
            db_mode: false,
            max_width: None,
            compare_slots: self.compare_slots(g, item),
            in_items_tab: false,
            empty_flask_slots: 0.0,
        };
        self.read(|e| crate::item_tooltip::add_item_stat_differences(tooltip, g, e, item, &args));
    }

    /// :4973-4978
    fn compare_slots(&self, g: &Gfx, item: &Rc<Item>) -> Vec<CompareSlot> {
        if item.base.is_none() || item.base.is_some_and(|b| b.flask.is_some() || b.tincture.is_some()) {
            return Vec::new();
        }
        let second = self.read(|e| e.active_item_set().use_second_weapon_set);
        let weapon_set = if second { 2.0 } else { 1.0 };
        let slots = self.slots.borrow();
        let mut out = Vec::new();
        self.read(|e| {
            for (name, s) in slots.iter() {
                if e.is_item_valid_for_slot(item, name)
                    && !s.inactive.get()
                    && s.weapon_set.get().is_none_or(|w| w == weapon_set)
                    && s.drop.base.shown.get(g)
                {
                    out.push(s.compare_info());
                }
            }
        });
        out
    }

    /// `ItemsTab:PopulateSlots()` (:1705-1709). `pairs(self.slots)`: the order is `orderedSlots` here, Weapon 1 before Weapon 2 (R13: upstream's order is the process's).
    pub fn populate_slots(&self, g: &Gfx) {
        let slots: Vec<_> = self.slots.borrow().values().cloned().collect();
        for s in slots {
            s.populate(g);
        }
    }

    /// `ItemsTab:UpdateSockets()` (:1712-1731).
    pub fn update_sockets(&self, _g: &Gfx) {
        let spec = self.read(Engine::spec_rc);
        let mut active: Vec<u32> = Vec::new();
        for (id, slot) in self.sockets.borrow().iter() {
            if spec.alloc_nodes.contains_key(id) {
                active.push(*id);
                slot.inactive.set(false);
            } else {
                slot.inactive.set(true);
            }
        }
        active.sort_unstable();
        let mut last = self.slot(BASE_SLOTS[BASE_SLOTS.len() - 1]);
        for (i, id) in active.iter().enumerate() {
            let s = self.sockets.borrow()[id].clone();
            *s.label.borrow_mut() = format!("Socket #{}", i + 1);
            last = s;
        }
        *self.last_slot.borrow_mut() = Some(last);
    }

    /// `ItemsTab:SetActiveItemSet(id)` (:1644-1670).
    pub fn set_active_item_set(&self, g: &mut Gfx, id: f64) {
        self.edit(|e| {
            e.set_active_item_set(id);
            Ok(())
        });
        self.sync_gear_slots();
        self.build().build_flag.set(true);
        self.populate_slots(g);
        self.build().sync_loadouts_pub(g);
    }

    /// `PopulateSlots`, `AddUndoState` after a character import (ImportTab.lua:1164-1165, :1481-1482) with what the engine changed in the slots and the sets.
    pub fn after_import(&self, g: &mut Gfx) {
        self.sync_gear_slots();
        self.populate_slots(g);
        self.update_sockets(g);
        self.update_item_list(g);
        self.add_undo_state(false);
    }

    /// `slot.selItemId`, `slot.active` of the set.
    fn sync_gear_slots(&self) {
        let set = self.read(|e| e.active_item_set().clone());
        for (name, s) in self.slots.borrow().iter() {
            if s.node_id.is_none() {
                let en = set.slots.get(name);
                s.sel_item_id.set(en.map_or(0.0, |x| x.item_id));
                let active = en.map(|x| x.active).filter(|a| *a);
                s.active.set(active);
                if let Some(c) = s.activate.borrow().as_ref() {
                    c.state.set(active == Some(true));
                }
            }
        }
    }

    /// `AddItem(item, noAutoEquip, index)` (:1739-1777): the auto equip asks `slot:IsShown()`.
    pub fn add_item(&self, g: &Gfx, item: &Item, no_auto_equip: bool, index: Option<usize>) -> f64 {
        self.read(Engine::touch_misc);
        let was_new = item.id.is_none() || self.read(|e| item.id.and_then(|i| e.item_by_id(i)).is_none());
        let id = self.edit(|e| e.add_item(item, index));
        if was_new && item.id.is_none() && !no_auto_equip {
            let slots: Vec<_> = self.slots.borrow().values().cloned().collect();
            for s in slots {
                let parsed = self.read(|e| e.item_by_id(id)).expect("the new item");
                if s.node_id.is_none()
                    && s.sel_item_id.get() == 0.0
                    && s.is_shown(g)
                    && self.read(|e| e.is_item_valid_for_slot(&parsed, &s.slot_name))
                {
                    s.set_sel_item_id(id);
                    break;
                }
            }
        }
        id
    }

    /// `new("Item"):Item(raw)` + `NormaliseQuality` + `AddItem(newItem, true)` + `AddForbiddenJewelCounterpart` (ItemSlotControl.lua:133-137 and the lists).
    pub fn add_new_from_raw(&self, g: &mut Gfx, raw: &str) -> f64 {
        let mut it = Item::parse_with(raw, None, false);
        it.normalise_quality();
        let id = self.add_item(g, &it, true, None);
        self.add_forbidden_counterpart(g, id);
        id
    }

    /// `ItemsTab:AddForbiddenJewelCounterpart(item)` (:1781-1824).
    pub fn add_forbidden_counterpart(&self, g: &Gfx, id: f64) {
        let Some(item) = self.read(|e| e.item_by_id(id)) else { return };
        let other = match item.title.as_deref() {
            Some("Forbidden Flame") => "Forbidden Flesh",
            Some("Forbidden Flesh") => "Forbidden Flame",
            _ => return,
        };
        let (Some(vl), Some(v)) = (&item.variant_list, item.variant) else {
            return;
        };
        let Some(name) = vl.get(v as usize - 1) else { return };
        let exists = self.read(|e| {
            e.items().values().any(|o| {
                o.title.as_deref() == Some(other)
                    && o.variant_list
                        .as_ref()
                        .zip(o.variant)
                        .is_some_and(|(l, x)| l.get(x as usize - 1) == Some(name))
            })
        });
        if exists {
            return;
        }
        let Some(db) = unique_db_items().iter().find(|u| u.title.as_deref() == Some(other)).cloned() else {
            return;
        };
        let mut new_item = Item::parse_with(&db.raw, None, false);
        let Some(i) = new_item.variant_list.as_ref().and_then(|l| l.iter().position(|n| n == name)) else {
            return;
        };
        new_item.variant = Some((i + 1) as f64);
        new_item.build_and_parse_raw();
        new_item.normalise_quality();
        self.add_item(g, &new_item, true, None);
    }

    /// `ItemsTab:UpdateForbiddenJewelCounterpart(oldItem, item)` (:1827-1854).
    pub fn update_forbidden_counterpart(&self, old: Option<&Rc<Item>>, item: &Item) {
        let other = match item.title.as_deref() {
            Some("Forbidden Flame") => "Forbidden Flesh",
            Some("Forbidden Flesh") => "Forbidden Flame",
            _ => return,
        };
        let Some(old) = old.filter(|o| o.title == item.title) else { return };
        let name_of = |i: &Item| {
            i.variant_list
                .as_ref()
                .zip(i.variant)
                .and_then(|(l, v)| l.get(v as usize - 1).cloned())
        };
        let (Some(old_name), Some(new_name)) = (name_of(old), name_of(item)) else {
            return;
        };
        if old_name == new_name {
            return;
        }
        let matches: Vec<Rc<Item>> = self.read(|e| {
            e.items()
                .values()
                .filter(|o| o.id != item.id && o.title.as_deref() == Some(other) && name_of(o).as_deref() == Some(old_name.as_str()))
                .cloned()
                .collect()
        });
        for o in matches {
            if let Some(i) = o.variant_list.as_ref().and_then(|l| l.iter().position(|n| *n == new_name)) {
                let mut c = (*o).clone();
                c.variant = Some((i + 1) as f64);
                c.build_and_parse_raw();
                self.edit(|e| e.add_item(&c, None));
                self.build().build_flag.set(true);
            }
        }
    }

    /// `ItemsTab:AddDisplayItem(noAutoEquip)` (:1857-1870).
    pub fn add_display_item(&self, g: &mut Gfx, no_auto_equip: bool) {
        let Some(item) = self.display_item.borrow().clone() else { return };
        let old = item.id.and_then(|i| self.read(|e| e.item_by_id(i)));
        let id = self.add_item(g, &item, no_auto_equip, None);
        self.set_display_item(g, None);
        self.update_forbidden_counterpart(old.as_ref(), &item);
        self.add_forbidden_counterpart(g, id);
        self.populate_slots(g);
        self.add_undo_state(false);
        self.build().build_flag.set(true);
        self.update_item_list(g);
    }

    /// `ItemsTab:GetEquippedSlotForItem(item)` (:2585-2599): the slot and the set when it is another's.
    pub fn equipped_slot(&self, id: f64) -> Option<(Rc<ItemSlotControl>, Option<f64>)> {
        let active = self.read(Engine::active_item_set_id);
        let sets: Vec<_> = self.read(|e| e.item_sets().to_vec());
        for s in self.slots.borrow().values() {
            if !s.inactive.get() {
                if s.sel_item_id.get() == id {
                    return Some((s.clone(), None));
                }
                for set in &sets {
                    if set.id != active && set.slots.get(&s.slot_name).is_some_and(|x| x.item_id == id) {
                        return Some((s.clone(), Some(set.id)));
                    }
                }
            }
        }
        None
    }

    /// `ItemsTab:SortItemList()` (:1873-1907).
    pub fn sort_item_list(&self) {
        let slot_order: std::collections::HashMap<String, usize> =
            self.slots.borrow().keys().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let (mut order, set_order) = self.read(|e| (e.item_order(), e.item_sets().iter().map(|s| s.id).collect::<Vec<_>>()));
        let items: std::collections::HashMap<u64, Rc<Item>> = self.read(|e| e.items().iter().map(|(k, v)| (*k, v.clone())).collect());
        let equipped: std::collections::HashMap<u64, Option<(String, Option<f64>)>> = order
            .iter()
            .map(|id| (*id as u64, self.equipped_slot(*id).map(|(s, st)| (s.slot_name.clone(), st))))
            .collect();
        let pos = |x: f64| set_order.iter().position(|s| *s == x).unwrap_or(0);
        pob_engine::lua::lua_sort(&mut order, &mut |a, b| {
            let (ia, ib) = (&items[&(*a as u64)], &items[&(*b as u64)]);
            let (pa, pb) = (ia.get_primary_slot(), ib.get_primary_slot());
            if pa != pb {
                let Some(oa) = slot_order.get(&pa) else { return false };
                let Some(ob) = slot_order.get(&pb) else { return true };
                return oa < ob;
            }
            match (&equipped[&(*a as u64)], &equipped[&(*b as u64)]) {
                (Some((sa, ea)), Some((sb, eb))) => {
                    if sa != sb {
                        return slot_order[sa] < slot_order[sb];
                    }
                    match (ea, eb) {
                        (Some(_), None) => return false,
                        (None, Some(_)) => return true,
                        (Some(x), Some(y)) => return pos(*x) < pos(*y),
                        (None, None) => {}
                    }
                }
                (Some(_), None) => return true,
                (None, Some(_)) => return false,
                (None, None) => {}
            }
            ia.name.as_bytes() < ib.name.as_bytes()
        });
        self.edit(|e| {
            e.set_item_order(order);
            Ok(())
        });
        self.add_undo_state(false);
    }

    /// `ItemsTab:DeleteItem(item, deferUndoState)` (:1910-1957).
    pub fn delete_item(&self, g: &Gfx, id: f64, defer: bool) {
        self.read(Engine::touch_misc);
        let mut flag = false;
        for s in self.slots.borrow().values() {
            if s.sel_item_id.get() == id {
                s.sel_item_id.set(0.0);
                flag = true;
            }
        }
        if self.read(|e| e.item_sets().iter().any(|set| set.slots.values().any(|x| x.item_id == id))) {
            flag = true;
        }
        self.edit(|e| e.delete_item(id));
        if flag {
            self.build().build_flag.set(true);
        }
        if !defer {
            self.populate_slots(g);
            self.add_undo_state(false);
        }
    }

    /// UndoHandler.lua:41-47
    pub(crate) fn undo_g(&self, g: &mut Gfx) {
        if self.undo.undo.borrow().len() >= 2 {
            let top = self.undo.undo.borrow_mut().remove(0);
            self.undo.redo.borrow_mut().insert(0, top);
            let s = self.undo.undo.borrow_mut().remove(0);
            self.restore(g, &s);
            self.add_undo_state(true);
        }
    }

    /// UndoHandler.lua:50-55
    pub(crate) fn redo_g(&self, g: &mut Gfx) {
        if !self.undo.redo.borrow().is_empty() {
            let s = self.undo.redo.borrow_mut().remove(0);
            self.restore(g, &s);
            self.add_undo_state(true);
        }
    }

    /// `ItemsTab:RestoreUndoState` (:5098-5115).
    fn restore(&self, g: &mut Gfx, st: &ItemsUndoState) {
        self.edit(|e| e.restore_items_undo_state(st));
        self.sync_gear_slots();
        self.populate_slots(g);
        self.update_item_list(g);
    }

    /// `self.socketViewer` for its tooltip parts.
    pub(crate) fn with_socket_viewer(&self, f: impl FnOnce(&mut crate::tree_view::PassiveTreeView, &Engine)) {
        let root = crate::launch::launch().cfg.script_path.clone();
        let mut slot = self.socket_viewer.borrow_mut();
        let v = slot.get_or_insert_with(|| crate::tree_view::PassiveTreeView::new(&root));
        self.read(|e| f(v, e));
    }

    /// `self.socketViewer` over `ItemSlotHelper.DrawViewer` (ItemSlotHelper.lua:10-37).
    pub(crate) fn draw_socket_viewer(&self, f: &mut Frame<'_>, node_id: u32, x: f64, y: f64, w: f64, h: f64) {
        let spec = self.read(Engine::spec_rc);
        let Some(node) = spec.node(node_id) else { return };
        sg::set_draw_layer(f, None, 15);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, x, y, w + 2.0, h + 2.0);
        let root = crate::launch::launch().cfg.script_path.clone();
        let mut slot = self.socket_viewer.borrow_mut();
        let v = slot.get_or_insert_with(|| crate::tree_view::PassiveTreeView::new(&root));
        v.zoom = 17.0;
        let size = v.art(&root, &spec.tree).size;
        let scale = size / (w.min(h) * v.zoom);
        let tn = node.tree_node();
        v.zoom_x = -tn.x.expect("x") / scale;
        v.zoom_y = -tn.y.expect("y") / scale;
        sg::set_viewport(f, Some([x + 1.0, y + 1.0, w, h]));
        let mut events = InputEvents::default();
        let b = self.build();
        v.draw(
            f,
            &root,
            &b.engine,
            &b.build_flag,
            &b.mod_flag,
            &Rect {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            },
            &mut events,
        );
        sg::set_draw_layer(f, None, 30);
        sg::set_draw_color_a(f, 1.0, 1.0, 1.0, 0.2);
        sg::draw_rect(f, (w / 2.0).floor() - 1.0, 0.0, 2.0, h);
        sg::draw_rect(f, 0.0, (h / 2.0).floor() - 1.0, w, 2.0);
        sg::set_viewport(f, None);
        sg::set_draw_layer(f, None, 0);
    }

    // ------------------------------------------------------------------------------------------ the item list

    /// `ItemListClass:UpdateLoadoutList()`.
    fn update_loadout_list(&self, g: &Gfx) -> bool {
        let mut list = vec!["Any Loadout".to_string(), "Current Loadout".to_string(), "Unused Items".to_string()];
        let b = self.build();
        for v in b.c().build_loadouts.list.borrow().iter() {
            let val = v.label.clone().unwrap_or_default();
            if !["^7^7Loadouts:", "^7^7-----", "^7^7New Loadout", "^7^7Sync", "^7^7Help >>"].contains(&val.as_str()) && !list.contains(&val)
            {
                list.push(val);
            }
        }
        for s in self.read(|e| e.item_sets().to_vec()) {
            let title = s.title.unwrap_or_else(|| "Default".to_string());
            if !list.contains(&title) {
                list.push(title);
            }
        }
        let key = list.join("\0");
        if self.loadout_list_key.borrow().as_deref() == Some(key.as_str()) {
            return false;
        }
        *self.loadout_list_key.borrow_mut() = Some(key);
        let f = self.loadout_filter.borrow().clone().expect("loadoutFilter");
        let sel = f.sel_index.get();
        let value = f
            .list
            .borrow()
            .get(sel.wrapping_sub(1))
            .and_then(|v| v.label.clone())
            .unwrap_or_else(|| "Any Loadout".to_string());
        f.set_list(g, Some(list.iter().map(|s| DropItem::str(s)).collect()));
        f.sel_index.set(list.iter().position(|x| *x == value).map_or(1, |i| i + 1));
        true
    }

    /// `self.list = itemsTab.itemOrderList`: the list of `Any Loadout` is that table upstream, before the first `Draw` too.
    pub fn sync_item_rows(&self) {
        let f = self.loadout_filter.borrow().clone().expect("loadoutFilter");
        if f.sel_index.get() <= 1 {
            let l = self.item_list.borrow().clone().expect("itemList");
            *l.list.borrow_mut() = self.read(Engine::item_order).into_iter().map(ItemRow::Id).collect();
        }
    }

    /// `ItemListClass:UpdateList()`.
    pub fn update_item_list(&self, g: &Gfx) {
        self.update_loadout_list(g);
        let f = self.loadout_filter.borrow().clone().expect("loadoutFilter");
        let sel_filter = f.sel_index.get();
        let filter_val = f
            .list
            .borrow()
            .get(sel_filter - 1)
            .and_then(|v| v.label.clone())
            .unwrap_or_else(|| "Any Loadout".to_string());
        let l = self.item_list.borrow().clone().expect("itemList");
        let selected = l.sel_value.borrow().clone();
        let order = self.read(Engine::item_order);
        let rows: Vec<ItemRow> = if sel_filter == 1 || filter_val == "Any Loadout" {
            l.is_mutable.set(true);
            order.iter().map(|i| ItemRow::Id(*i)).collect()
        } else {
            l.is_mutable.set(false);
            let unused = sel_filter == 3 || filter_val == "Unused Items";
            let current = sel_filter == 2 || filter_val == "Current Loadout";
            let (set, spec_jewels) = self.read(|e| {
                let sets = e.item_sets();
                let specs = &e.model().tree.specs;
                if current {
                    return (
                        Some(e.active_item_set().clone()),
                        Some(pob_calc::pob_build::active_spec(e.model()).clone()),
                    );
                }
                if unused {
                    return (None, None);
                }
                let title = pob_items::lua::gsub(&filter_val, "^%[[^%]]+%]%s*", "", None);
                let mut set = sets.iter().find(|s| s.title.as_deref().unwrap_or("Default") == title).cloned();
                let mut spec = specs.iter().find(|s| s.title.as_deref().unwrap_or("Default") == title).cloned();
                if set.is_none() && sets.len() == 1 {
                    set = Some(sets[0].clone());
                }
                if spec.is_none() && specs.len() == 1 {
                    spec = Some(specs[0].clone());
                }
                (set, spec)
            });
            let mut out = Vec::new();
            for id in &order {
                if unused {
                    if self.equipped_slot(*id).is_none()
                        && self.find_equipped_abyss(*id, false).is_none()
                        && self.find_socketed_jewel(*id, false).is_none()
                    {
                        out.push(ItemRow::Id(*id));
                    }
                } else {
                    let mut hit = set.as_ref().is_some_and(|s| s.slots.values().any(|x| x.item_id == *id));
                    if !hit && let Some(sp) = &spec_jewels {
                        hit = sp.jewels.0.iter().any(|(n, j)| *j == *id && sp.nodes.contains(n));
                    }
                    if hit {
                        out.push(ItemRow::Id(*id));
                    }
                }
            }
            out
        };
        let idx = selected.as_ref().and_then(|v| rows.iter().position(|x| x == v));
        *l.list.borrow_mut() = rows;
        l.sel_index.set(idx.map(|i| i + 1));
        *l.sel_value.borrow_mut() = idx.and(selected);
    }

    /// `ItemListClass:FindSocketedJewel(jewelId, excludeActiveSpec)`.
    pub fn find_socketed_jewel(&self, id: f64, exclude_active: bool) -> Option<String> {
        self.read(|e| {
            if e.item_by_id(id).is_none_or(|i| i.ty.as_deref() != Some("Jewel")) {
                return None;
            }
            let m = e.model();
            let active = m.tree.active_spec as usize;
            let mut equip = None;
            let mut match_active = false;
            for (i, sp) in m.tree.specs.iter().enumerate().rev() {
                for (n, j) in &sp.jewels.0 {
                    if *j == id && sp.nodes.contains(n) {
                        if exclude_active && (i + 1 == active || match_active) {
                            equip = None;
                            match_active = true;
                        } else {
                            equip = Some(sp.title.clone().unwrap_or_else(|| "Default".to_string()));
                        }
                    }
                }
            }
            equip
        })
    }

    /// `ItemListClass:FindEquippedAbyssJewel(jewelId, excludeActiveSet)`.
    pub fn find_equipped_abyss(&self, id: f64, exclude_active: bool) -> Option<String> {
        self.read(|e| {
            if e.item_by_id(id)
                .is_none_or(|i| i.base.and_then(|b| b.sub_type.as_deref()) != Some("Abyss"))
            {
                return None;
            }
            let active = e.active_item_set_id();
            let mut equip = None;
            let mut match_active = false;
            for set in e.item_sets() {
                for x in set.slots.values() {
                    if x.item_id == id {
                        if exclude_active && (set.id == active || match_active) {
                            equip = None;
                            match_active = true;
                        } else {
                            equip = Some(set.title.clone().unwrap_or_else(|| "Default".to_string()));
                        }
                    }
                }
            }
            equip
        })
    }

    /// The double click of a row of the list (ItemListControl.lua:324-331).
    fn open_list_item(&self, g: &mut Gfx, id: f64) {
        let Some(item) = self.read(|e| e.item_by_id(id)) else { return };
        let mut new_item = Item::parse_with(&item.build_raw(), None, false);
        new_item.id = Some(id);
        self.set_display_item(g, Some(new_item));
    }

    /// Ctrl+click on a row of the list or the database: where the item goes (`slotName`, ItemListControl.lua:302-314).
    fn target_slot(&self, g: &Gfx, item: &Item) -> Option<String> {
        let mut name = item.get_primary_slot();
        let c = self.slots.borrow().get(&name).cloned()?;
        if c.weapon_set.get() == Some(1.0) && self.read(|e| e.active_item_set().use_second_weapon_set) {
            name = format!("{name} Swap");
        }
        if sg::is_key_down(g, "SHIFT") {
            let alt = name.replacen('1', "2", usize::MAX);
            if self.read(|e| e.is_item_valid_for_slot(item, &alt)) {
                name = alt;
            }
        }
        Some(name)
    }

    /// `ItemsTab:Draw(viewPort, inputEvents)` (:1472-1623).
    #[allow(clippy::too_many_lines)]
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let a = &self.anchor.base;
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        let sbh = self.scroll_bar_h.borrow().clone().expect("scrollBarH");
        let sbv = self.scroll_bar_v.borrow().clone().expect("scrollBarV");
        sbh.base.width.set(view_port.width);
        sbh.base.x.set(view_port.x);
        sbh.base.y.set(view_port.y + view_port.height - 18.0);
        sbv.base.height.set(view_port.height - 18.0);
        sbv.base.x.set(view_port.x + view_port.width - 18.0);
        sbv.base.y.set(view_port.y);
        {
            let last = self.last_slot.borrow().clone().expect("lastSlot");
            let mut max_y = last.get_pos(f).1 + 24.0;
            let ad = self.anchor_display_item.borrow().clone().expect("anchor");
            let mut max_x = ad.get_pos(f).0 + 462.0;
            if self.display_item.borrow().is_some() {
                let an = self.tooltip_anchor.borrow().clone().expect("tooltip anchor");
                let (x, y) = an.get_pos(f);
                let (tw, th) = self.display_tooltip.borrow_mut().get_dynamic_size(f, view_port);
                max_y = max_y.max(y + th + 4.0);
                max_x = max_x.max(x + tw + 80.0);
            }
            let content_h = max_y - view_port.y;
            let content_w = max_x - view_port.x;
            let mut v = content_h > view_port.height;
            let h = content_w > view_port.width - if v { 20.0 } else { 0.0 };
            if h {
                v = content_h > view_port.height - 20.0;
            }
            sbv.set_content_dimension(f, content_h, view_port.height - if h { 20.0 } else { 0.0 });
            sbh.set_content_dimension(f, content_w, view_port.width - if v { 20.0 } else { 0.0 });
            match self.snap_h.take() {
                Some("RIGHT") => sbh.set_offset(sbh.offset_max.get().unwrap_or(0.0)),
                Some("LEFT") => sbh.set_offset(0.0),
                _ => {}
            }
            self.max_y.set(if h { sbh.base.y.get(f) } else { view_port.y + view_port.height });
        }
        a.x.set(view_port.x - sbh.offset.get());
        a.y.set(view_port.y - sbv.offset.get());

        let keys: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in keys {
            let ctrl = sg::is_key_down(f, "CTRL");
            match key.as_str() {
                "v" if ctrl => {
                    if let Some(text) = f.paste() {
                        self.create_display_item_from_raw(f, &text, true);
                    }
                }
                "e" => {
                    let over = self.host.get_mouse_over_control(f);
                    let slot = over.and_then(|c| self.slots.borrow().values().find(|s| crate::control::same(&***s, &*c)).cloned());
                    if let Some(s) = slot
                        && s.sel_item_id.get() != 0.0
                    {
                        self.open_list_item(f, s.sel_item_id.get());
                    }
                }
                "z" if ctrl => {
                    self.undo_g(f);
                    self.build().build_flag.set(true);
                }
                "y" if ctrl => {
                    self.redo_g(f);
                    self.build().build_flag.set(true);
                }
                "f" if ctrl => {
                    let sel = self.host.sel();
                    let (u, r) = (
                        self.unique_db.borrow().clone().expect("db"),
                        self.rare_db.borrow().clone().expect("db"),
                    );
                    let is = |d: &ItemDb| sel.as_ref().is_some_and(|c| crate::control::same(&*d.search, &**c));
                    let db = self.select_db.borrow().clone().expect("selectDB");
                    let (sel_u, sel_r) = (is(&u), is(&r));
                    if sel_u || (db.is_shown(f) && !sel_r && db.sel_index.get() == 2) {
                        self.host.select_control(f, Some(r.search.clone()));
                        db.sel_index.set(2);
                    } else {
                        self.host.select_control(f, Some(u.search.clone()));
                        db.sel_index.set(1);
                    }
                }
                "d" if ctrl => {
                    let v = !self.read(Engine::items_show_stat_differences);
                    self.edit(|e| {
                        e.set_items_show_stat_differences(v);
                        Ok(())
                    });
                    self.build().build_flag.set(true);
                }
                _ => {}
            }
        }
        self.host.process_controls_input(f, input_events, view_port);
        let ups: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyUp { key } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in ups {
            let vertical = sbv.is_mouse_over(f) == Some(true) || !sbh.is_shown(f);
            if ScrollBarControl::is_scroll_down_key(&key) {
                if vertical {
                    sbv.scroll(1.0);
                } else {
                    sbh.scroll(1.0);
                }
            } else if ScrollBarControl::is_scroll_up_key(&key) {
                if vertical {
                    sbv.scroll(-1.0);
                } else {
                    sbh.scroll(-1.0);
                }
            }
        }
        main().draw_background(f, view_port);
        let (sets, active) = self.read(|e| (e.item_sets().to_vec(), e.active_item_set_id()));
        let list: Vec<DropItem> = sets
            .iter()
            .map(|s| DropItem::str(s.title.as_deref().unwrap_or("Default")))
            .collect();
        if let Some(i) = sets.iter().position(|s| s.id == active) {
            self.set_select.sel_index.set(i + 1);
        }
        self.set_select.set_list(f, Some(list));

        if self.display_item.borrow().is_some() {
            let an = self.tooltip_anchor.borrow().clone().expect("tooltip anchor");
            let (x, y) = an.get_pos(f);
            self.display_tooltip.borrow_mut().draw(f, x, y, None, view_port);
            // :1582-1601 toggle mods
            let (cx, cy) = f.get_cursor_pos();
            let bounds: Vec<((u8, usize), Rect)> = self
                .display_tooltip
                .borrow()
                .lines
                .iter()
                .filter_map(|l| Some((l.mod_line?, l.bounds?)))
                .collect();
            for (id, b) in bounds {
                if cx >= b.x && cx <= b.x + b.width && cy >= b.y && cy <= b.y + b.height {
                    sg::set_draw_color_a(f, 1.0, 1.0, 1.0, 0.15);
                    sg::draw_rect(f, b.x, b.y, b.width, b.height);
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    let hit = ipairs(input_events).find_map(|(i, e)| match e {
                        InputEvent::KeyDown { key, .. } if key.contains("BUTTON") => Some(i),
                        _ => None,
                    });
                    if let Some(i) = hit {
                        input_events[i] = None;
                        self.toggle_display_mod_line(f, id);
                    }
                }
            }
        }
        self.update_sockets(f);
        let il = self.item_list.borrow().clone().expect("itemList");
        let portrait = main().portrait_mode.get();
        let il_ref: ControlRef = il.clone();
        let sm: ControlRef = self.set_manage.borrow().clone().expect("setManage");
        if portrait {
            let last: ControlRef = self.last_slot.borrow().clone().expect("lastSlot");
            il.base
                .set_anchor("TOPRIGHT", Some(Rc::downgrade(&last)), "BOTTOMRIGHT", Some((0.0, 40.0)), false);
        } else {
            il.base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&sm)), "TOPRIGHT", Some((20.0, 20.0)), false);
        }
        let other = if portrait { &sm } else { &il_ref };
        let y = if portrait { 0.0 } else { -20.0 };
        self.craft_display_item.borrow().as_ref().expect("craft").base.set_anchor(
            "TOPLEFT",
            Some(Rc::downgrade(other)),
            "TOPRIGHT",
            Some((20.0, y)),
            false,
        );
        self.anchor_display_item.borrow().as_ref().expect("anchor").base.set_anchor(
            "TOPLEFT",
            Some(Rc::downgrade(other)),
            "TOPRIGHT",
            Some((20.0, y)),
            false,
        );
        let loadout_changed = self.update_loadout_list(f);
        let rev = self.read(Engine::output_revision);
        if loadout_changed || self.last_output_revision.get() != Some(rev) {
            self.last_output_revision.set(Some(rev));
            self.update_item_list(f);
        }
        for d in [&self.unique_db, &self.rare_db] {
            d.borrow().as_ref().expect("db").before_draw(f);
        }
        let sel = self.host.sel();
        self.host.draw_controls(f, view_port, sel.as_ref());
        if sbh.is_shown(f) {
            sbh.draw(f, view_port, false);
        }
        if sbv.is_shown(f) {
            sbv.draw(f, view_port, false);
        }
        // `self.controls.specSelect:SetList(self.build.treeTab:GetSpecList())`
        let spec_list: Vec<DropItem> = self.read(Engine::spec_list_labels).iter().map(|s| DropItem::str(s)).collect();
        let spec_select = self.spec_select.borrow().clone().expect("specSelect");
        spec_select.set_list(f, Some(spec_list));
    }

    /// `AddItemSetTooltip(tooltip, itemSet)` (:4042-4051).
    pub(crate) fn add_item_set_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, set_id: f64) {
        let set = self.read(|e| e.item_sets().iter().find(|s| s.id == set_id).cloned());
        let Some(set) = set else { return };
        for s in self.slots.borrow().values() {
            if s.node_id.is_none()
                && let Some(item) = self.read(|e| e.item_by_id(set.slots.get(&s.slot_name).map_or(0.0, |x| x.item_id)))
            {
                tooltip.add_line(g, 16.0, Some(&format!("^7{}: {}{}", s.label.borrow(), cc(&item.rarity), item.name)));
            }
        }
    }

    /// The Manage popup (:2666-2676).
    pub(crate) fn open_item_set_manage_popup(&self, g: &mut Gfx) {
        note_not_ported(
            "Classes/ItemSetListControl.lua:89-110, SharedItemSetListControl.lua:18-112: drag between the set lists, Delete and Rename of a shared set",
        );
        let (set_list, shared_list) = new_manage_lists(g, self.me.clone());
        *self.set_list.borrow_mut() = Rc::downgrade(&set_list);
        *self.shared_set_list.borrow_mut() = Rc::downgrade(&shared_list);
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert("setList".into(), set_list);
        controls.insert("sharedList".into(), shared_list);
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(0.0, 260.0, 90.0, 20.0), "Done".to_string(), |_g| main().close_popup()),
        );
        main().open_popup(g, 630.0, 290.0, "Manage Item Sets", controls, None, None, None, None);
    }

    /// `RenameSet(itemSet, addOnName)` of the set list (ItemSetListControl.lua:48-74). `what`: an id, or the set to make when the popup saves.
    pub(crate) fn rename_set_popup(&self, g: &mut Gfx, what: RenameItemSet) {
        let title = match &what {
            RenameItemSet::Existing(id) => self.read(|e| e.item_sets().iter().find(|s| s.id == *id).and_then(|s| s.title.clone())),
            RenameItemSet::Copy(id) => self.read(|e| e.item_sets().iter().find(|s| s.id == *id).and_then(|s| s.title.clone())),
            RenameItemSet::New => None,
        };
        let save: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let s1 = save.clone();
        let edit = EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 350.0, 20.0),
            EditArgs {
                init: title.clone(),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _: bool| {
                    let ok = buf.chars().any(|c| !c.is_whitespace());
                    if let Some(b) = s1.borrow().as_ref() {
                        b.base.enabled.set(ok);
                    }
                })),
                ..EditArgs::default()
            },
        );
        let me = self.me.clone();
        let e2 = edit.clone();
        let btn = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
            let t = me.upgrade().expect("itemsTab");
            let name = e2.buf.borrow().clone();
            let l = t.set_list.borrow().upgrade();
            let added = match &what {
                RenameItemSet::Existing(id) => {
                    t.edit(|e| {
                        e.rename_item_set(*id, &name);
                        Ok(())
                    });
                    None
                }
                RenameItemSet::New => Some(t.edit(|e| Ok(e.new_item_set(Some(&name), None)))),
                RenameItemSet::Copy(id) => Some(t.edit(|e| Ok(e.copy_item_set(*id, &name)))),
            };
            t.undo.mod_flag.set(true);
            if let (Some(id), Some(l)) = (added, l) {
                l.list.borrow_mut().push(id);
                l.sel_index.set(Some(l.list.borrow().len()));
                *l.sel_value.borrow_mut() = Some(id);
            }
            t.add_undo_state(false);
            t.build().sync_loadouts_pub(g);
            main().close_popup();
        });
        btn.base.enabled.set(false);
        *save.borrow_mut() = Some(btn.clone());
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "label".into(),
            LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter name for this item set:".to_string()),
        );
        controls.insert("edit".into(), edit);
        controls.insert("save".into(), btn);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
        );
        main().open_popup(
            g,
            370.0,
            100.0,
            if title.is_some() { "Rename" } else { "Set Name" },
            controls,
            Some("save"),
            Some("edit"),
            Some("cancel"),
            None,
        );
    }
}

pub enum RenameItemSet {
    Existing(f64),
    New,
    Copy(f64),
}

// ---------------------------------------------------------------------------------------------- the lists

struct ItemListHooks(Weak<ItemsTab>);

impl ListHooks<ItemRow> for ItemListHooks {
    /// ItemListControl.lua:250-266
    fn get_row_value(&self, _l: &ListControl<ItemRow>, _g: &Gfx, column: usize, _index: usize, v: &ItemRow) -> String {
        let (1, ItemRow::Id(id)) = (column, v) else { return String::new() };
        let t = self.0.upgrade().expect("itemsTab");
        let Some(item) = t.read(|e| e.item_by_id(*id)) else {
            return String::new();
        };
        let used = t.find_equipped_abyss(*id, true).or_else(|| t.find_socketed_jewel(*id, true));
        let tail = match used {
            Some(u) => format!("  ^9(Used in '{u}')"),
            None => match t.equipped_slot(*id) {
                None => "  ^9(Unused)".to_string(),
                Some((_, Some(set))) => {
                    let title = t.read(|e| e.item_sets().iter().find(|s| s.id == set).and_then(|s| s.title.clone()));
                    format!("  ^9(Used in '{}')", title.as_deref().unwrap_or("Default"))
                }
                Some((_, None)) => String::new(),
            },
        };
        format!("{}{}{tail}", cc(&item.rarity), item.name)
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    /// :268-277
    fn add_value_tooltip(&self, _l: &ListControl<ItemRow>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, v: &ItemRow) {
        if !main().popups.borrow().is_empty() {
            tooltip.clear(false);
            return;
        }
        let ItemRow::Id(id) = v else { return };
        let t = self.0.upgrade().expect("itemsTab");
        let Some(item) = t.read(|e| e.item_by_id(*id)) else { return };
        let key = (
            Rc::as_ptr(&item) as usize,
            sg::is_key_down(g, "SHIFT"),
            t.read(Engine::output_revision),
        );
        if tooltip.check_for_update(key) {
            t.add_tooltip(tooltip, g, &item, None, false);
        }
    }
    fn get_drag_value(&self, _l: &ListControl<ItemRow>, _index: usize, v: &ItemRow) -> (String, ItemRow) {
        ("Item".to_string(), v.clone())
    }
    fn can_receive_drag(&self, _l: &ListControl<ItemRow>, drag_type: &str, _v: &ItemRow) -> bool {
        drag_type == "Item"
    }
    /// :283-293
    fn receive_drag(&self, l: &ListControl<ItemRow>, g: &mut Gfx, drag_type: &str, v: &ItemRow) {
        if drag_type != "Item" {
            return;
        }
        let t = self.0.upgrade().expect("itemsTab");
        let raw = match v {
            ItemRow::Id(id) => t.read(|e| e.item_by_id(*id)).map(|i| i.raw.clone()),
            ItemRow::Obj(i) => Some(i.raw.clone()),
        };
        let Some(raw) = raw else { return };
        let mut it = Item::parse_with(&raw, None, false);
        it.normalise_quality();
        let id = t.add_item(g, &it, true, l.sel_drag_index.get());
        t.add_forbidden_counterpart(g, id);
        t.populate_slots(g);
        t.add_undo_state(false);
        t.update_item_list(g);
    }
    /// :295-297 (the list was reordered by `ListControl`).
    fn on_order_change(&self, l: &ListControl<ItemRow>, _old: usize, _new: usize) {
        let t = self.0.upgrade().expect("itemsTab");
        let order: Vec<f64> = l
            .list
            .borrow()
            .iter()
            .filter_map(|r| if let ItemRow::Id(i) = r { Some(*i) } else { None })
            .collect();
        t.edit(|e| {
            e.set_item_order(order);
            Ok(())
        });
        t.add_undo_state(false);
    }
    /// :299-333
    fn on_sel_click(&self, l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow, double_click: bool) -> Option<bool> {
        let ItemRow::Id(id) = v else { return None };
        let t = self.0.upgrade().expect("itemsTab");
        let item = t.read(|e| e.item_by_id(*id))?;
        if sg::is_key_down(g, "CTRL") {
            if let Some(name) = t.target_slot(g, &item) {
                let s = t.slot(&name);
                s.set_sel_item_id(if s.sel_item_id.get() == *id { 0.0 } else { *id });
                t.populate_slots(g);
                t.add_undo_state(false);
                t.build().build_flag.set(true);
            }
        } else if double_click {
            l.sel_dragging.set(false);
            t.open_list_item(g, *id);
            return Some(false);
        }
        None
    }
    /// :335-338
    fn on_sel_copy(&self, _l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow) {
        let ItemRow::Id(id) = v else { return };
        let t = self.0.upgrade().expect("itemsTab");
        if let Some(item) = t.read(|e| e.item_by_id(*id)) {
            g.copy(&item.build_raw().replace('\n', "\r\n"));
        }
    }
    /// :340-378
    fn on_sel_delete(&self, _l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow) {
        let ItemRow::Id(id) = v else { return };
        let id = *id;
        let t = self.0.upgrade().expect("itemsTab");
        let Some(item) = t.read(|e| e.item_by_id(id)) else { return };
        let w = self.0.clone();
        let del = Rc::new(move |g: &mut Gfx| {
            let t = w.upgrade().expect("itemsTab");
            t.delete_item(g, id, false);
            let l = t.item_list.borrow().clone().expect("itemList");
            l.sel_index.set(None);
            *l.sel_value.borrow_mut() = None;
            t.update_item_list(g);
        });
        let set_title = |s: f64| {
            t.read(|e| e.item_sets().iter().find(|x| x.id == s).and_then(|x| x.title.clone()))
                .unwrap_or_else(|| "Default".to_string())
        };
        let confirm = |g: &mut Gfx, msg: String, del: Rc<dyn Fn(&mut Gfx)>| {
            main().open_confirm_popup(g, "Delete Item", &msg, "Delete", del, None);
        };
        if let Some((slot, set)) = t.equipped_slot(id) {
            let in_set = set.map(|s| format!(" in set '{}'", set_title(s))).unwrap_or_default();
            confirm(
                g,
                format!(
                    "{} is currently equipped in {}{in_set}.\nAre you sure you want to delete it?",
                    item.name,
                    slot.label.borrow()
                ),
                del,
            );
        } else if let Some(set) = t.find_equipped_abyss(id, true) {
            confirm(
                g,
                format!(
                    "{} is currently equipped in an Abyssal Socket in set '{set}'.\nAre you sure you want to delete it?",
                    item.name
                ),
                del,
            );
        } else if let Some(tree) = t.find_socketed_jewel(id, true) {
            confirm(
                g,
                format!(
                    "{} is currently equipped in passive tree '{tree}'.\nAre you sure you want to delete it?",
                    item.name
                ),
                del,
            );
        } else {
            del(g);
        }
    }
    fn has_on_hover_key_up(&self) -> bool {
        true
    }
    /// :380-387
    fn on_hover_key_up(&self, l: &ListControl<ItemRow>, g: &mut Gfx, key: &str) {
        if key == "F1"
            && let Some(ItemRow::Id(id)) = l.get_hover_index(g).and_then(|i| l.list.borrow().get(i - 1).cloned())
        {
            let t = self.0.upgrade().expect("itemsTab");
            if let Some(item) = t.read(|e| e.item_by_id(id)) {
                wiki_open_item(g, &item);
            }
        }
    }
}

struct SharedListHooks(Weak<ItemsTab>);

impl ListHooks<ItemRow> for SharedListHooks {
    fn get_row_value(&self, _l: &ListControl<ItemRow>, _g: &Gfx, column: usize, _index: usize, v: &ItemRow) -> String {
        match (column, v) {
            (1, ItemRow::Obj(i)) => format!("{}{}", cc(&i.rarity), i.name),
            _ => String::new(),
        }
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    fn add_value_tooltip(&self, _l: &ListControl<ItemRow>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, v: &ItemRow) {
        if !main().popups.borrow().is_empty() {
            tooltip.clear(false);
            return;
        }
        let ItemRow::Obj(item) = v else { return };
        let t = self.0.upgrade().expect("itemsTab");
        let key = (
            Rc::as_ptr(item) as usize,
            sg::is_key_down(g, "SHIFT"),
            t.read(Engine::output_revision),
        );
        if tooltip.check_for_update(key) {
            t.add_tooltip(tooltip, g, item, None, false);
        }
    }
    fn get_drag_value(&self, _l: &ListControl<ItemRow>, _index: usize, v: &ItemRow) -> (String, ItemRow) {
        ("Item".to_string(), v.clone())
    }
    fn can_receive_drag(&self, _l: &ListControl<ItemRow>, drag_type: &str, _v: &ItemRow) -> bool {
        drag_type == "Item"
    }
    /// SharedItemListControl.lua:52-61
    fn receive_drag(&self, l: &ListControl<ItemRow>, _g: &mut Gfx, drag_type: &str, v: &ItemRow) {
        if drag_type != "Item" {
            return;
        }
        let t = self.0.upgrade().expect("itemsTab");
        let (raw, has_id) = match v {
            ItemRow::Id(id) => (t.read(|e| e.item_by_id(*id)).map(|i| i.build_raw()), true),
            ItemRow::Obj(i) => (Some(i.build_raw()), i.id.is_some()),
        };
        let Some(raw) = raw else { return };
        let mut it = Item::parse_with(&raw, None, false);
        if !has_id {
            it.normalise_quality();
        }
        {
            let mut list = l.list.borrow_mut();
            // `t_insert(self.list, self.selDragIndex or #self.list, newItem)`
            let at = l
                .sel_drag_index
                .get()
                .map_or(list.len().saturating_sub(1), |i| i - 1)
                .min(list.len());
            list.insert(at, ItemRow::Obj(Rc::new(it)));
        }
        sync_shared_items(l);
    }
    /// :63-68
    fn on_sel_click(&self, l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow, double_click: bool) -> Option<bool> {
        if double_click && let ItemRow::Obj(i) = v {
            self.0.upgrade().expect("itemsTab").create_display_item_from_raw(g, &i.raw, true);
            l.sel_dragging.set(false);
        }
        None
    }
    fn on_sel_copy(&self, _l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow) {
        if let ItemRow::Obj(i) = v {
            g.copy(&i.build_raw().replace('\n', "\r\n"));
        }
    }
    /// :74-80
    fn on_sel_delete(&self, _l: &ListControl<ItemRow>, g: &mut Gfx, index: usize, v: &ItemRow) {
        let ItemRow::Obj(i) = v else { return };
        let w = self.0.clone();
        main().open_confirm_popup(
            g,
            "Delete Item",
            &format!("Are you sure you want to remove '{}' from the shared item list?", i.name),
            "Delete",
            Rc::new(move |_g: &mut Gfx| {
                let t = w.upgrade().expect("itemsTab");
                let l = t.shared_list.borrow().clone().expect("sharedItemList");
                l.list.borrow_mut().remove(index - 1);
                l.sel_index.set(None);
                *l.sel_value.borrow_mut() = None;
                sync_shared_items(&l);
            }),
            None,
        );
    }
}

/// `main.sharedItemList` is this list upstream: `main` saves the texts.
fn sync_shared_items(l: &ListControl<ItemRow>) {
    *main().shared_item_list.borrow_mut() = l
        .list
        .borrow()
        .iter()
        .filter_map(|r| if let ItemRow::Obj(i) = r { Some(i.raw.clone()) } else { None })
        .collect();
}

struct SetListHooks(Weak<ItemsTab>);

impl ListHooks<f64> for SetListHooks {
    fn get_row_value(&self, _l: &ListControl<f64>, _g: &Gfx, column: usize, _index: usize, id: &f64) -> String {
        if column != 1 {
            return String::new();
        }
        let t = self.0.upgrade().expect("itemsTab");
        t.read(|e| {
            let title = e.item_sets().iter().find(|s| s.id == *id).and_then(|s| s.title.clone());
            format!(
                "{}{}",
                title.as_deref().unwrap_or("Default"),
                if *id == e.active_item_set_id() { "  ^9(Current)" } else { "" }
            )
        })
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    fn add_value_tooltip(&self, _l: &ListControl<f64>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, id: &f64) {
        tooltip.clear(false);
        self.0.upgrade().expect("itemsTab").add_item_set_tooltip(tooltip, g, *id);
    }
    fn on_order_change(&self, l: &ListControl<f64>, _old: usize, _new: usize) {
        let t = self.0.upgrade().expect("itemsTab");
        let order = l.list.borrow().clone();
        t.edit(|e| {
            e.set_item_set_order(&order);
            Ok(())
        });
        t.undo.mod_flag.set(true);
    }
    fn on_sel_click(&self, _l: &ListControl<f64>, g: &mut Gfx, _index: usize, id: &f64, double_click: bool) -> Option<bool> {
        let t = self.0.upgrade().expect("itemsTab");
        if double_click && *id != t.read(Engine::active_item_set_id) {
            t.set_active_item_set(g, *id);
            t.add_undo_state(false);
        }
        None
    }
    /// ItemSetListControl.lua:123-138
    fn on_sel_delete(&self, l: &ListControl<f64>, g: &mut Gfx, index: usize, id: &f64) {
        if l.list.borrow().len() <= 1 {
            return;
        }
        let t = self.0.upgrade().expect("itemsTab");
        let title = t.read(|e| e.item_sets().iter().find(|s| s.id == *id).and_then(|s| s.title.clone()));
        let w = self.0.clone();
        let id = *id;
        main().open_confirm_popup(
            g,
            "Delete Item Set",
            &format!(
                "Are you sure you want to delete '{}'?\nThis will not delete any items used by the set.",
                title.as_deref().unwrap_or("Default")
            ),
            "Delete",
            Rc::new(move |g: &mut Gfx| {
                let t = w.upgrade().expect("itemsTab");
                let l = t.set_list.borrow().upgrade().expect("setList");
                l.list.borrow_mut().remove(index - 1);
                t.edit(|e| {
                    e.delete_item_set(id);
                    Ok(())
                });
                l.sel_index.set(None);
                *l.sel_value.borrow_mut() = None;
                if id == t.read(|e| e.model().active_item_set) {
                    let next = l.list.borrow()[index.saturating_sub(1).max(1) - 1];
                    t.set_active_item_set(g, next);
                }
                t.add_undo_state(false);
                t.build().sync_loadouts_pub(g);
            }),
            None,
        );
    }
    fn on_sel_key_down(&self, _l: &ListControl<f64>, g: &mut Gfx, _index: usize, id: &f64, key: &str) {
        if key == "F2" {
            self.0
                .upgrade()
                .expect("itemsTab")
                .rename_set_popup(g, RenameItemSet::Existing(*id));
        }
    }
}

struct SharedSetHooks(Weak<ItemsTab>);

impl ListHooks<usize> for SharedSetHooks {
    fn get_row_value(&self, _l: &ListControl<usize>, _g: &Gfx, column: usize, index: usize, _v: &usize) -> String {
        if column != 1 {
            return String::new();
        }
        main().shared_item_set_list.borrow()[index - 1]
            .title
            .clone()
            .unwrap_or_else(|| "Default".to_string())
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    fn add_value_tooltip(&self, _l: &ListControl<usize>, g: &Gfx, tooltip: &mut Tooltip, index: usize, _v: &usize) {
        tooltip.clear(false);
        let t = self.0.upgrade().expect("itemsTab");
        let m = main();
        let sets = m.shared_item_set_list.borrow();
        let Some(set) = sets.get(index - 1) else { return };
        for s in t.slots.borrow().values() {
            if s.node_id.is_none()
                && let Some(raw) = set.slots.get(&s.slot_name)
            {
                let item = Item::parse_with(raw, None, false);
                tooltip.add_line(g, 16.0, Some(&format!("^7{}: {}{}", s.label.borrow(), cc(&item.rarity), item.name)));
            }
        }
    }
}

fn new_manage_lists(g: &mut Gfx, tab: Weak<ItemsTab>) -> (Rc<ListControl<f64>>, Rc<ListControl<usize>>) {
    let t = tab.upgrade().expect("itemsTab");
    let order: Vec<f64> = t.read(|e| e.item_sets().iter().map(|s| s.id).collect());
    let set_list = ListControl::new(
        None,
        rect(-155.0, 50.0, 300.0, 200.0),
        16.0,
        Some("VERTICAL"),
        true,
        order,
        false,
        Box::new(SetListHooks(tab.clone())),
    );
    let r: ControlRef = set_list.clone();
    let w = Rc::downgrade(&set_list);
    let t1 = tab.clone();
    let copy = ButtonControl::new(
        anchor("BOTTOMLEFT", &r, "TOP"),
        rect(2.0, -4.0, 60.0, 18.0),
        "Copy".to_string(),
        move |g| {
            let l = w.upgrade().expect("list");
            let id = *l.sel_value.borrow().as_ref().expect("selValue");
            t1.upgrade().expect("itemsTab").rename_set_popup(g, RenameItemSet::Copy(id));
        },
    );
    let w = Rc::downgrade(&set_list);
    copy.base
        .enabled
        .set_fn(move |_| w.upgrade().expect("list").sel_value.borrow().is_some());
    let cr: ControlRef = copy.clone();
    let w = Rc::downgrade(&set_list);
    let delete = ButtonControl::new(
        anchor("LEFT", &cr, "RIGHT"),
        rect(4.0, 0.0, 60.0, 18.0),
        "Delete".to_string(),
        move |g| {
            let l = w.upgrade().expect("list");
            let v = *l.sel_value.borrow();
            if let (Some(i), Some(v)) = (l.sel_index.get(), v) {
                l.hooks.on_sel_delete(&l, g, i, &v);
            }
        },
    );
    let w = Rc::downgrade(&set_list);
    delete.base.enabled.set_fn(move |_| {
        let l = w.upgrade().expect("list");
        let selected = l.sel_value.borrow().is_some();
        selected && l.list.borrow().len() > 1
    });
    let w = Rc::downgrade(&set_list);
    let t1 = tab.clone();
    let rename = ButtonControl::new(
        anchor("BOTTOMRIGHT", &r, "TOP"),
        rect(-2.0, -4.0, 60.0, 18.0),
        "Rename".to_string(),
        move |g| {
            let l = w.upgrade().expect("list");
            let id = *l.sel_value.borrow().as_ref().expect("selValue");
            t1.upgrade().expect("itemsTab").rename_set_popup(g, RenameItemSet::Existing(id));
        },
    );
    let w = Rc::downgrade(&set_list);
    rename
        .base
        .enabled
        .set_fn(move |_| w.upgrade().expect("list").sel_value.borrow().is_some());
    let rr: ControlRef = rename.clone();
    let t1 = tab.clone();
    let new_btn = ButtonControl::new(
        anchor("RIGHT", &rr, "LEFT"),
        rect(-4.0, 0.0, 60.0, 18.0),
        "New".to_string(),
        move |g| {
            t1.upgrade().expect("itemsTab").rename_set_popup(g, RenameItemSet::New);
        },
    );
    set_list.host.add("copy", copy);
    set_list.host.add("delete", delete);
    set_list.host.add("rename", rename);
    set_list.host.add("new", new_btn);

    let n = main().shared_item_set_list.borrow().len();
    let shared = ListControl::new(
        None,
        rect(155.0, 50.0, 300.0, 200.0),
        16.0,
        Some("VERTICAL"),
        true,
        (0..n).collect(),
        false,
        Box::new(SharedSetHooks(tab)),
    );
    *shared.default_text.borrow_mut() = Some("^x7F7F7FThis is a list of item sets that will be shared\nbetween all of your builds.\nYou can add sets to this list by dragging them\nfrom the build's set list.".to_string());
    let _ = g;
    (set_list, shared)
}

// ---------------------------------------------------------------------------------------------- ItemDBControl

thread_local! {
    static UNIQUE_DB: std::cell::OnceCell<Vec<Rc<Item>>> = const { std::cell::OnceCell::new() };
    static RARE_DB: std::cell::OnceCell<Vec<Rc<Item>>> = const { std::cell::OnceCell::new() };
    static UNIQUE_LOAD: RefCell<Option<DbLoad>> = const { RefCell::new(None) };
    static RARE_LOAD: RefCell<Option<DbLoad>> = const { RefCell::new(None) };
}

/// A database on its way: what `loadItemDBs` has yet to parse and the list so far.
struct DbLoad {
    raws: std::vec::IntoIter<String>,
    list: IndexMap<String, Rc<Item>>,
}

fn unique_raws() -> Vec<String> {
    let j: serde_json::Value = pob_data::read_json("uniques.json").unwrap_or_else(|e| panic!("{e}"));
    let mut out = Vec::new();
    for (_, raws) in j["byType"].as_object().into_iter().flatten() {
        for raw in raws.as_array().into_iter().flatten() {
            out.push(raw.as_str().unwrap_or("").to_string());
        }
    }
    out
}

fn rare_raws() -> Vec<String> {
    pob_data::read_json("rares.json").unwrap_or_else(|e| panic!("{e}"))
}

/// Main.lua:165-173.
fn parse_unique(raw: &str, list: &mut IndexMap<String, Rc<Item>>) {
    let it = Item::parse_with(&crate::common::sanitise_text(raw), Some("UNIQUE"), true);
    if it.base.is_some() {
        list.insert(it.name.clone(), Rc::new(it));
    }
}

/// Main.lua:180-195.
fn parse_rare(raw: &str, list: &mut IndexMap<String, Rc<Item>>) {
    let mut it = Item::parse_with(&crate::common::sanitise_text(raw), Some("RARE"), true);
    let Some(base) = it.base else { return };
    if it.crafted {
        if let Some(imp) = &base.implicit
            && it.implicit_mod_lines.is_empty()
        {
            for (i, line) in imp.split('\n').filter(|l| !l.is_empty()).enumerate() {
                it.implicit_mod_lines.push(pob_items::ModLine {
                    line: line.to_string(),
                    mod_tags: base.implicit_mod_types.get(i).cloned().unwrap_or_default(),
                    ..Default::default()
                });
            }
        }
        it.craft();
    }
    list.insert(it.name.clone(), Rc::new(it));
}

type DbCell = std::thread::LocalKey<std::cell::OnceCell<Vec<Rc<Item>>>>;
type LoadCell = std::thread::LocalKey<RefCell<Option<DbLoad>>>;

/// One database, until it is done or `deadline` has passed. `true`: done.
fn advance_db(
    db: &'static DbCell,
    load: &'static LoadCell,
    raws: fn() -> Vec<String>,
    parse: fn(&str, &mut IndexMap<String, Rc<Item>>),
    deadline: Option<std::time::Instant>,
) -> bool {
    if db.with(|d| d.get().is_some()) {
        return true;
    }
    let done = load.with_borrow_mut(|l| {
        let l = l.get_or_insert_with(|| DbLoad {
            raws: raws().into_iter(),
            list: IndexMap::new(),
        });
        loop {
            let Some(raw) = l.raws.next() else {
                return Some(std::mem::take(&mut l.list));
            };
            parse(&raw, &mut l.list);
            if deadline.is_some_and(|t| std::time::Instant::now() >= t) {
                return None;
            }
        }
    });
    let Some(list) = done else { return false };
    load.with_borrow_mut(|l| *l = None);
    db.with(|d| {
        let _ = d.set(list.into_values().collect());
    });
    true
}

/// `onFrameFuncs["LoadItems"]` (Main.lua:292-304, `loadItemDBs` :163-200): the databases parsed over the first frames of the program, the uniques first; parsed up to `budget` now, all of them
/// without one. `true`: both are ready. A reader that comes earlier finishes its database itself ([`unique_db_items`], [`rare_db_items`]).
pub fn load_item_dbs_step(budget: Option<std::time::Duration>) -> bool {
    let deadline = budget.map(|b| std::time::Instant::now() + b);
    advance_db(&UNIQUE_DB, &UNIQUE_LOAD, unique_raws, parse_unique, deadline)
        && advance_db(&RARE_DB, &RARE_LOAD, rare_raws, parse_rare, deadline)
}

/// `main.uniqueDB.list` (Main.lua:164-176): one item per name.
pub fn unique_db_items() -> Vec<Rc<Item>> {
    advance_db(&UNIQUE_DB, &UNIQUE_LOAD, unique_raws, parse_unique, None);
    UNIQUE_DB.with(|d| d.get().expect("the unique database").clone())
}

/// `main.rareDB.list` (Main.lua:180-197).
pub fn rare_db_items() -> Vec<Rc<Item>> {
    advance_db(&RARE_DB, &RARE_LOAD, rare_raws, parse_rare, None);
    RARE_DB.with(|d| d.get().expect("the rare database").clone())
}

/// `ItemDBControl`: the list and the controls of its filters.
pub struct ItemDb {
    pub list: Rc<ListControl<ItemRow>>,
    pub db_type: &'static str,
    pub tab: Weak<ItemsTab>,
    pub slot: Rc<DropDownControl>,
    pub ty: Rc<DropDownControl>,
    pub sort: Option<Rc<DropDownControl>>,
    pub league: Option<Rc<DropDownControl>>,
    pub requirement: Option<Rc<DropDownControl>>,
    pub obtainable: Option<Rc<DropDownControl>>,
    pub search: Rc<EditControl>,
    pub search_mode: Rc<DropDownControl>,
    pub list_build_flag: Rc<Cell<bool>>,
    pub list_output_revision: Cell<Option<u64>>,
    pub sort_mode: RefCell<String>,
    pub leagues_loaded: Cell<bool>,
    league_list: RefCell<Vec<String>>,
    type_list: RefCell<Vec<String>>,
    /// `item.measuredPower`.
    power: RefCell<std::collections::HashMap<usize, f64>>,
}

const SLOT_LIST: [&str; 14] = [
    "Any slot",
    "Weapon 1",
    "Weapon 2",
    "Helmet",
    "Body Armour",
    "Gloves",
    "Boots",
    "Amulet",
    "Ring",
    "Belt",
    "Jewel",
    "Flask",
    "Graft 1",
    "Graft 2",
];

fn db_sort_items() -> Vec<DropItem> {
    pob_data::rules::rules()
        .power_stat_list
        .as_array()
        .expect("powerStatList")
        .iter()
        .filter(|e| !e.get("ignoreForItems").and_then(serde_json::Value::as_bool).unwrap_or(false))
        .map(|e| {
            let label = e.get("label").and_then(serde_json::Value::as_str).unwrap_or("");
            let mode = e
                .get("itemField")
                .or_else(|| e.get("stat"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut it = DropItem::table(&format!("Sort by {label}"));
            it.data = Some(Rc::new(mode));
            it
        })
        .collect()
}

struct DbHooks(Weak<ItemsTab>);

impl ListHooks<ItemRow> for DbHooks {
    fn get_row_value(&self, _l: &ListControl<ItemRow>, _g: &Gfx, column: usize, _index: usize, v: &ItemRow) -> String {
        match (column, v) {
            (1, ItemRow::Obj(i)) => format!("{}{}", cc(&i.rarity), i.name),
            _ => String::new(),
        }
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    fn add_value_tooltip(&self, _l: &ListControl<ItemRow>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, v: &ItemRow) {
        if !main().popups.borrow().is_empty() {
            tooltip.clear(false);
            return;
        }
        let ItemRow::Obj(item) = v else { return };
        let t = self.0.upgrade().expect("itemsTab");
        let key = (
            Rc::as_ptr(item) as usize,
            sg::is_key_down(g, "SHIFT"),
            t.read(Engine::output_revision),
        );
        if tooltip.check_for_update(key) {
            t.add_tooltip(tooltip, g, item, None, true);
        }
    }
    fn get_drag_value(&self, _l: &ListControl<ItemRow>, _index: usize, v: &ItemRow) -> (String, ItemRow) {
        ("Item".to_string(), v.clone())
    }
    /// ItemDBControl.lua:350-387
    fn on_sel_click(&self, l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow, double_click: bool) -> Option<bool> {
        let ItemRow::Obj(item) = v else { return None };
        let t = self.0.upgrade().expect("itemsTab");
        if sg::is_key_down(g, "CTRL") {
            let mut it = Item::parse_with(&item.raw, None, false);
            it.normalise_quality();
            let id = t.add_item(g, &it, true, None);
            let parsed = t.read(|e| e.item_by_id(id)).expect("the new item");
            if let Some(name) = t.target_slot(g, &parsed) {
                t.slot(&name).set_sel_item_id(id);
            }
            t.add_forbidden_counterpart(g, id);
            t.populate_slots(g);
            t.add_undo_state(false);
            t.build().build_flag.set(true);
            t.update_item_list(g);
        } else if double_click {
            l.sel_dragging.set(false);
            t.create_display_item_from_raw(g, &item.raw, true);
            return Some(false);
        }
        None
    }
    fn on_sel_copy(&self, _l: &ListControl<ItemRow>, g: &mut Gfx, _index: usize, v: &ItemRow) {
        if let ItemRow::Obj(i) = v {
            g.copy(&i.raw.replace('\n', "\r\n"));
        }
    }
    fn has_on_hover_key_up(&self) -> bool {
        true
    }
    fn on_hover_key_up(&self, l: &ListControl<ItemRow>, g: &mut Gfx, key: &str) {
        if key == "F1"
            && let Some(ItemRow::Obj(i)) = l.get_hover_index(g).and_then(|i| l.list.borrow().get(i - 1).cloned())
        {
            wiki_open_item(g, &i);
        }
    }
}

impl ItemDb {
    /// `ItemDBClass:ItemDBControl(anchor, rect, itemsTab, db, dbType)`.
    fn new(
        g: &mut Gfx,
        a: Option<crate::control::Anchor>,
        r: crate::control::RectProps,
        tab: Weak<ItemsTab>,
        db_type: &'static str,
    ) -> Rc<Self> {
        // `ItemDBClass:Draw(viewPort)` does not pass `noTooltip` on to `ListControl.Draw`
        let list = ListControl::new(
            a,
            r,
            16.0,
            Some("VERTICAL"),
            false,
            Vec::new(),
            true,
            Box::new(DbHooks(tab.clone())),
        );
        let flag = Rc::new(Cell::new(true));
        let lr: ControlRef = list.clone();
        let mk = |g: &mut Gfx, an, x, y, w, items: Vec<&str>| {
            let f = flag.clone();
            DropDownControl::new(
                g,
                an,
                rect(x, y, w, 18.0),
                items.iter().map(|s| DropItem::str(s)).collect(),
                Some(Rc::new(move |_g: &mut Gfx, _i: usize, _v: &DropItem| f.set(true))),
                None,
                false,
            )
        };
        let base_y = if db_type == "RARE" { -22.0 } else { -62.0 };
        let slot = mk(g, anchor("BOTTOMLEFT", &lr, "TOPLEFT"), 0.0, base_y, 179.0, SLOT_LIST.to_vec());
        let sr: ControlRef = slot.clone();
        let ty = mk(
            g,
            anchor("LEFT", &sr, "RIGHT"),
            2.0,
            0.0,
            179.0,
            vec!["Any type", "Armour", "Jewellery", "One Handed Melee", "Two Handed Melee"],
        );
        list.host.add("slot", slot.clone());
        list.host.add("type", ty.clone());
        let (mut sort, mut league, mut requirement, mut obtainable) = (None, None, None, None);
        if db_type == "UNIQUE" {
            let t2 = tab.clone();
            let s = DropDownControl::new(
                g,
                anchor("BOTTOMLEFT", &lr, "TOPLEFT"),
                rect(0.0, base_y + 20.0, 179.0, 18.0),
                db_sort_items(),
                Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
                    let mode = v
                        .data
                        .as_ref()
                        .and_then(|d| d.downcast_ref::<String>())
                        .cloned()
                        .unwrap_or_default();
                    let t = t2.upgrade().expect("itemsTab");
                    let d = t.unique_db.borrow().clone().expect("db");
                    *d.sort_mode.borrow_mut() = mode;
                    d.list_build_flag.set(true);
                })),
                None,
                false,
            );
            s.check_dropped_width(g, true);
            let sr: ControlRef = s.clone();
            let l = mk(g, anchor("LEFT", &sr, "RIGHT"), 2.0, 0.0, 179.0, vec!["Any league", "No league"]);
            let rq = mk(
                g,
                anchor("LEFT", &sr, "BOTTOMLEFT"),
                0.0,
                11.0,
                179.0,
                vec!["Any requirements", "Current level", "Current attributes", "Current useable"],
            );
            let rr: ControlRef = rq.clone();
            let ob = mk(
                g,
                anchor("LEFT", &rr, "RIGHT"),
                2.0,
                0.0,
                179.0,
                vec![
                    "Obtainable",
                    "Any source",
                    "Unobtainable",
                    "Vendor Recipe",
                    "Upgraded",
                    "Boss Item",
                    "Corruption",
                    "Core Drop Pool",
                ],
            );
            list.host.add("sort", s.clone());
            list.host.add("league", l.clone());
            list.host.add("requirement", rq.clone());
            list.host.add("obtainable", ob.clone());
            sort = Some(s);
            league = Some(l);
            requirement = Some(rq);
            obtainable = Some(ob);
        }
        let f = flag.clone();
        let search = EditControl::new(
            g,
            anchor("BOTTOMLEFT", &lr, "TOPLEFT"),
            rect(0.0, -2.0, 258.0, 18.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Search".to_string()),
                filter: Some("%c".to_string()),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g: &mut Gfx, _b: &str, _: bool| f.set(true))),
                clearable: true,
                ..EditArgs::default()
            },
        );
        let sr: ControlRef = search.clone();
        let search_mode = mk(
            g,
            anchor("LEFT", &sr, "RIGHT"),
            2.0,
            0.0,
            100.0,
            vec!["Anywhere", "Names", "Modifiers"],
        );
        list.host.add("search", search.clone());
        list.host.add("searchMode", search_mode.clone());
        Rc::new(ItemDb {
            list,
            db_type,
            tab,
            slot,
            ty,
            sort,
            league,
            requirement,
            obtainable,
            search,
            search_mode,
            list_build_flag: flag,
            list_output_revision: Cell::new(None),
            sort_mode: RefCell::new("NAME".to_string()),
            leagues_loaded: Cell::new(false),
            league_list: RefCell::new(vec!["Any league".to_string(), "No league".to_string()]),
            type_list: RefCell::new(
                ["Any type", "Armour", "Jewellery", "One Handed Melee", "Two Handed Melee"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            ),
            power: RefCell::new(std::collections::HashMap::new()),
        })
    }

    fn items(&self) -> Vec<Rc<Item>> {
        if self.db_type == "UNIQUE" {
            unique_db_items()
        } else {
            rare_db_items()
        }
    }

    /// `LoadLeaguesAndTypes` (:75-93).
    fn load_leagues_and_types(&self, g: &Gfx) {
        let mut leagues = std::collections::BTreeSet::new();
        let mut types = std::collections::BTreeSet::new();
        for item in self.items() {
            if let Some(l) = &item.league {
                for c in pob_items::lua::gmatch(l, " ?([%w ]+),?") {
                    leagues.insert(c[0].clone());
                }
            }
            if let Some(t) = &item.ty {
                types.insert(t.clone());
            }
        }
        self.league_list.borrow_mut().extend(leagues);
        self.type_list.borrow_mut().extend(types);
        if let Some(l) = &self.league {
            *l.list.borrow_mut() = self.league_list.borrow().iter().map(|s| DropItem::str(s)).collect();
        }
        *self.ty.list.borrow_mut() = self.type_list.borrow().iter().map(|s| DropItem::str(s)).collect();
        let _ = g;
        self.leagues_loaded.set(true);
    }

    /// `DoesItemMatchFilters(item)` (:95-213).
    #[allow(clippy::too_many_lines)]
    fn matches(&self, e: &Engine, item: &Item) -> bool {
        let slot_sel = self.slot.sel_index.get();
        if slot_sel > 1 {
            let primary = item.get_primary_slot();
            let want = SLOT_LIST[slot_sel - 1];
            if primary != want && pob_items::lua::gsub(&primary, " %d", "", None) != want {
                return false;
            }
        }
        let ty_sel = self.ty.sel_index.get();
        let ty = item.ty.as_deref().unwrap_or("");
        if ty_sel > 1 {
            match ty_sel {
                2 => {
                    if item.base.is_none_or(|b| b.armour.is_none()) {
                        return false;
                    }
                }
                3 => {
                    if !matches!(ty, "Amulet" | "Ring" | "Belt") {
                        return false;
                    }
                }
                4 | 5 => {
                    let info = pob_data::rules::rules().weapon_type_info.get(ty);
                    let ok = info.is_some_and(|i| {
                        i.melee == Some(true) && ((ty_sel == 4 && i.one_hand == Some(true)) || (ty_sel == 5 && i.one_hand != Some(true)))
                    });
                    if !ok {
                        return false;
                    }
                }
                _ => {
                    if self.type_list.borrow().get(ty_sel - 1).map(String::as_str) != Some(ty) {
                        return false;
                    }
                }
            }
        }
        let find = |s: &str, p: &str| pob_lua::find(s.as_bytes(), p.as_bytes(), None, false).ok().flatten().is_some();
        if let Some(l) = &self.league {
            let i = l.sel_index.get();
            if i > 1 {
                let name = self.league_list.borrow().get(i - 1).cloned().unwrap_or_default();
                if (i == 2 && item.league.is_some()) || (i > 2 && !item.league.as_deref().is_some_and(|x| find(x, &name))) {
                    return false;
                }
            }
        }
        if let Some(o) = &self.obtainable
            && o.sel_index.get() != 2
        {
            let i = o.sel_index.get();
            let source = item.source.as_deref().unwrap_or("");
            let obtainable = !(source == "No longer obtainable" || item.league.as_deref() == Some("Race Events"));
            if (i == 1 && !obtainable)
                || (i == 3 && obtainable)
                || (i == 4 && source != "Vendor Recipe")
                || (i == 5 && !find(source, "Upgraded from"))
                || (i == 6 && !find(source, "Drops from unique"))
                || (i == 7 && !find(source, "Vaal Orb"))
                || (i == 8 && item.source.is_some())
            {
                return false;
            }
        }
        if let Some(r) = &self.requirement {
            let i = r.sel_index.get();
            if i > 1 {
                if (i == 2 || i == 4) && item.requirements.level.is_some_and(|l| l > e.model().level) {
                    return false;
                }
                if i > 2 {
                    let o = |k| e.output_value(k).unwrap_or(0.0);
                    if item.requirements.str_mod.unwrap_or(0.0) > o("Str")
                        || item.requirements.dex_mod.unwrap_or(0.0) > o("Dex")
                        || item.requirements.int_mod.unwrap_or(0.0) > o("Int")
                    {
                        return false;
                    }
                }
            }
        }
        let buf = self.search.buf.borrow().clone();
        let mut search = String::new();
        for c in buf.to_ascii_lowercase().chars() {
            if "-.+[]$^%?*".contains(c) {
                search.push('%');
            }
            search.push(c);
        }
        if search.chars().any(|c| !c.is_whitespace()) {
            let mut found = false;
            let mode = self.search_mode.sel_index.get();
            let m = |s: &str, p: &str| crate::tabs::minion_list::match_or_pattern(s, p) == Some(true);
            if mode == 1 || mode == 2 {
                let mut name = item.name.to_ascii_lowercase();
                if item.explicit_mod_lines.iter().any(|l| l.new_mod_id.is_some()) {
                    name = format!("foulborn {name}");
                }
                found = m(&name, &search);
            }
            if mode == 1 || mode == 3 {
                found = found
                    || item.enchant_mod_lines.iter().any(|l| m(&l.line.to_ascii_lowercase(), &search))
                    || item.implicit_mod_lines.iter().any(|l| m(&l.line.to_ascii_lowercase(), &search));
                if !found {
                    for l in &item.explicit_mod_lines {
                        let mut s = l.line.to_ascii_lowercase();
                        if let Some(id) = &l.new_mod_id {
                            let new = pob_data::item_mods::item_exclusive()
                                .get(id)
                                .or_else(|| pob_data::item_mods::foulborn().get(id));
                            if let Some(n) = new {
                                s.push_str(&n.lines.join(" ").to_ascii_lowercase());
                            }
                        }
                        if m(&s, &search) {
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    let stripped = search.replace(' ', "");
                    found = item
                        .base_mod_list
                        .iter()
                        .flatten()
                        .any(|md| m(&md.name.to_ascii_lowercase(), &stripped));
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    /// `ListBuilder` (:247-301) in one frame.
    fn build_list(&self, g: &Gfx) {
        let t = self.tab.upgrade().expect("itemsTab");
        let mut list: Vec<Rc<Item>> = t.read(|e| self.items().into_iter().filter(|i| self.matches(e, i)).collect());
        let mode = self.sort_mode.borrow().clone();
        let stat = pob_calc::power::power_stat_list()
            .into_iter()
            .find(|p| p.stat.as_deref() == Some(mode.as_str()));
        let name_key = |s: &str| s.strip_prefix("The ").unwrap_or(s).to_string();
        if let Some(stat) = stat.filter(|_| mode != "NAME") {
            let mut power = self.power.borrow_mut();
            power.clear();
            let second = t.read(|e| e.active_item_set().use_second_weapon_set);
            let weapon_set = if second { 2.0 } else { 1.0 };
            let slots = t.slots.borrow().clone();
            for item in &list {
                let mut best = f64::NEG_INFINITY;
                for (name, slot) in &slots {
                    let ok = t.read(|e| e.is_item_valid_for_slot(item, name));
                    if ok && !slot.inactive.get() && slot.weapon_set.get().is_none_or(|w| w == weapon_set) && slot.drop.base.shown.get(g) {
                        let base = item.base.expect("base");
                        let ov = pob_calc::env::CalcOverride {
                            toggle_flask: base.flask.is_some().then(|| item.clone()),
                            toggle_tincture: base.tincture.is_some().then(|| item.clone()),
                            rep_slot_name: (base.flask.is_none() && base.tincture.is_none()).then(|| name.clone()),
                            rep_item: (base.flask.is_none() && base.tincture.is_none()).then(|| item.clone()),
                            ..Default::default()
                        };
                        let v = t.read(|e| e.item_override_power(ov, &stat)).unwrap_or_else(|er| panic!("{er}"));
                        best = best.max(v);
                    }
                }
                power.insert(Rc::as_ptr(item) as usize, best);
            }
            let p = &*power;
            pob_engine::lua::lua_sort(&mut list, &mut |a, b| {
                let (x, y) = (p[&(Rc::as_ptr(a) as usize)], p[&(Rc::as_ptr(b) as usize)]);
                if x != y {
                    return y < x;
                }
                name_key(&a.name).as_bytes() < name_key(&b.name).as_bytes()
            });
        } else {
            pob_engine::lua::lua_sort(&mut list, &mut |a, b| name_key(&a.name).as_bytes() < name_key(&b.name).as_bytes());
        }
        *self.list.list.borrow_mut() = list.into_iter().map(ItemRow::Obj).collect();
        *self.list.default_text.borrow_mut() = Some("^7No items found that match those filters.".to_string());
    }

    /// `ItemDBClass:Draw` before `ListControl.Draw` (:303-328): a hidden list is not drawn upstream, nor built.
    fn before_draw(&self, g: &Gfx) {
        if !self.list.is_shown(g) {
            return;
        }
        let t = self.tab.upgrade().expect("itemsTab");
        let rev = t.read(Engine::output_revision);
        if self.list_output_revision.get() != Some(rev) {
            self.list_build_flag.set(true);
        }
        if !self.leagues_loaded.get() {
            self.load_leagues_and_types(g);
        }
        if self.list_build_flag.get() {
            self.list_build_flag.set(false);
            self.list_output_revision.set(Some(rev));
            self.build_list(g);
        }
    }
}

// ---------------------------------------------------------------------------------------------- the constructor

impl ItemsTab {
    /// `ItemsTabClass:ItemsTab(build)` (:93-1246) + the state `Load` leaves.
    #[allow(clippy::too_many_lines)]
    pub fn new(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<ItemsTab> {
        let root = crate::launch::launch().cfg.script_path.clone();
        let foulborn_icon = ImageHandle::new();
        let _ = foulborn_icon.load(format!("{root}Assets/breachicon.png"), &[]);
        let tab = Rc::new_cyclic(|me: &Weak<ItemsTab>| {
            let anchor_c = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
            let w = me.clone();
            let t = move || w.upgrade().expect("itemsTab");
            let ar: ControlRef = anchor_c.clone();
            let t1 = t.clone();
            let set_select = DropDownControl::new(
                g,
                anchor("TOPLEFT", &ar, "TOPLEFT"),
                rect(96.0, 8.0, 216.0, 20.0),
                Vec::new(),
                Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                    let s = t1();
                    let id = s.read(|e| e.item_sets()[index - 1].id);
                    s.set_active_item_set(g, id);
                    s.add_undo_state(false);
                })),
                None,
                false,
            );
            set_select.enable_dropped_width.set(true);
            let t1 = t.clone();
            set_select.base.enabled.set_fn(move |_| t1().read(|e| e.item_sets().len() > 1));
            let t1 = t.clone();
            set_select.tip.set_tooltip_func(move |tooltip, g, (mode, index, _v)| {
                tooltip.clear(false);
                if mode == "HOVER" {
                    let s = t1();
                    let id = s.read(|e| e.item_sets().get(index - 1).map(|x| x.id));
                    if let Some(id) = id {
                        s.add_item_set_tooltip(tooltip, g, id);
                    }
                }
            });
            ItemsTab {
                host: ControlHost::new(),
                me: me.clone(),
                build: build.clone(),
                undo: UndoHandler::default(),
                anchor: anchor_c,
                set_select,
                slots: RefCell::new(IndexMap::new()),
                sockets: RefCell::new(IndexMap::new()),
                last_slot: RefCell::new(None),
                spec_select: RefCell::new(None),
                item_list: RefCell::new(None),
                loadout_filter: RefCell::new(None),
                loadout_list_key: RefCell::new(None),
                last_output_revision: Cell::new(None),
                shared_list: RefCell::new(None),
                select_db_label: RefCell::new(None),
                select_db: RefCell::new(None),
                unique_db: RefCell::new(None),
                rare_db: RefCell::new(None),
                anchor_display_item: RefCell::new(None),
                set_manage: RefCell::new(None),
                craft_display_item: RefCell::new(None),
                scroll_bar_h: RefCell::new(None),
                scroll_bar_v: RefCell::new(None),
                tooltip_anchor: RefCell::new(None),
                display_item: RefCell::new(None),
                display_tooltip: RefCell::new(Tooltip::new()),
                panel: RefCell::new(None),
                snap_h: Cell::new(None),
                max_y: Cell::new(0.0),
                socket_viewer: RefCell::new(None),
                set_list: RefCell::new(Weak::new()),
                shared_set_list: RefCell::new(Weak::new()),
                foulborn_icon: foulborn_icon.clone(),
                last_craft: Cell::new((3, 1, 1)),
                enchant_slot: Cell::new(1),
                anoint_enchant_slot: Cell::new(1),
            }
        });
        tab.display_tooltip.borrow_mut().max_width = Some(458.0);
        tab.build_controls(g);
        let weak = Rc::downgrade(&tab);
        tab.host.add("setSelect", tab.set_select.clone());
        let _ = weak;
        tab.after_load(g);
        tab
    }

    #[allow(clippy::too_many_lines)]
    fn build_controls(self: &Rc<Self>, g: &mut Gfx) {
        let me = Rc::downgrade(self);
        let t = {
            let me = me.clone();
            move || me.upgrade().expect("itemsTab")
        };
        let ar: ControlRef = self.anchor.clone();
        let sr: ControlRef = self.set_select.clone();
        self.host.add(
            "setLabel",
            LabelControl::new(anchor("RIGHT", &sr, "LEFT"), rect(-2.0, 0.0, 0.0, 16.0), "^7Item set:".to_string()),
        );
        let t1 = t.clone();
        let set_manage = ButtonControl::new(
            anchor("LEFT", &sr, "RIGHT"),
            rect(4.0, 0.0, 90.0, 20.0),
            "Manage...".to_string(),
            move |g| {
                t1().open_item_set_manage_popup(g);
            },
        );
        self.host.add("setManage", set_manage.clone());
        *self.set_manage.borrow_mut() = Some(set_manage.clone());
        let price = ButtonControl::new(
            anchor("TOPLEFT", &ar, "TOPLEFT"),
            rect(96.0, 32.0, 310.0, 20.0),
            "Trade for these items".to_string(),
            move |g| crate::modules::main::not_ported(g, "Classes/ItemsTab.lua:132 tradeQuery:PriceItem (the network, U8)"),
        );
        price.tip.set_tooltip_func(|tooltip, g, ()| {
            tooltip.clear(false);
            tooltip.add_line(g, 16.0, Some("^7Contains searches from the official trading site to help find"));
            tooltip.add_line(g, 16.0, Some("^7similar or better items for this build"));
        });
        self.host.add("priceDisplayItem", price);

        // :141-202 the slots
        let slot_anchor = BareControl::new(anchor("TOPLEFT", &ar, "TOPLEFT"), rect(96.0, 76.0, 310.0, 0.0));
        let sa: ControlRef = slot_anchor.clone();
        self.host.add("slotAnchor", slot_anchor.clone());
        let mut prev: ControlRef = slot_anchor.clone();
        let mut names: Vec<String> = Vec::new();
        let add = |this: &Rc<Self>, s: &Rc<ItemSlotControl>, prev: &mut ControlRef, names: &mut Vec<String>| {
            *prev = s.clone();
            this.slots.borrow_mut().insert(s.slot_name.clone(), s.clone());
            names.push(s.slot_name.clone());
            this.host.add(&format!("slot:{}", s.slot_name), s.clone());
        };
        for name in BASE_SLOTS {
            let slot = ItemSlotControl::new(g, &prev, me.clone(), name.to_string(), None, None);
            add(self, &slot, &mut prev, &mut names);
            if name.contains("Weapon") {
                slot.weapon_set.set(Some(1.0));
                let t1 = t.clone();
                slot.drop
                    .base
                    .shown
                    .set_fn(move |_| !t1().read(|e| e.active_item_set().use_second_weapon_set));
                let swap = ItemSlotControl::new(g, &prev, me.clone(), format!("{name} Swap"), Some(name), None);
                add(self, &swap, &mut prev, &mut names);
                swap.weapon_set.set(Some(2.0));
                let t1 = t.clone();
                swap.drop
                    .base
                    .shown
                    .set_fn(move |_| t1().read(|e| e.active_item_set().use_second_weapon_set));
                for i in 1..=6 {
                    let ab = ItemSlotControl::new(
                        g,
                        &prev,
                        me.clone(),
                        format!("{name} Swap Abyssal Socket {i}"),
                        Some(&format!("Abyssal #{i}")),
                        None,
                    );
                    add(self, &ab, &mut prev, &mut names);
                    ab.weapon_set.set(Some(2.0));
                    let (t1, w) = (t.clone(), Rc::downgrade(&ab));
                    ab.drop.base.shown.set_fn(move |_| {
                        !w.upgrade().expect("slot").inactive.get() && t1().read(|e| e.active_item_set().use_second_weapon_set)
                    });
                    swap.abyssal_socket_list.borrow_mut().push(ab);
                }
            } else if name == "Graft 1" || name == "Graft 2" {
                let t1 = t.clone();
                slot.drop
                    .base
                    .shown
                    .set_fn(move |_| t1().read(|e| e.spec_rc().tree_version.contains("3_27")));
            } else if name == "Ring 3" {
                let t1 = t.clone();
                slot.drop.base.shown.set_fn(move |_| {
                    t1().read(|e| e.result().env.mod_db.borrow().flag(None, &["AdditionalRingSlot"]).ok().flatten() == Some(true))
                });
            }
            if ["Weapon 1", "Weapon 2", "Helmet", "Gloves", "Body Armour", "Boots", "Belt"].contains(&name) {
                for i in 1..=6 {
                    let ab = ItemSlotControl::new(
                        g,
                        &prev,
                        me.clone(),
                        format!("{name} Abyssal Socket {i}"),
                        Some(&format!("Abyssal #{i}")),
                        None,
                    );
                    add(self, &ab, &mut prev, &mut names);
                    if name.contains("Weapon") {
                        ab.weapon_set.set(Some(1.0));
                        let (t1, w) = (t.clone(), Rc::downgrade(&ab));
                        ab.drop.base.shown.set_fn(move |_| {
                            !w.upgrade().expect("slot").inactive.get() && !t1().read(|e| e.active_item_set().use_second_weapon_set)
                        });
                    }
                    slot.abyssal_socket_list.borrow_mut().push(ab);
                }
            }
        }
        // :204-218 the tree dropdown
        let t1 = t.clone();
        let spec_select = DropDownControl::new(
            g,
            anchor("TOPLEFT", &prev, "BOTTOMLEFT"),
            rect(0.0, 8.0, 216.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                let b = s.build();
                if s.read(|e| index <= e.model().tree.specs.len()) {
                    b.mod_flag.set(true);
                    let tab = b.tree_tab.borrow().clone().expect("treeTab");
                    tab.set_active_spec(g, index);
                }
            })),
            None,
            false,
        );
        let w = Rc::downgrade(&spec_select);
        spec_select
            .base
            .enabled
            .set_fn(move |_| w.upgrade().expect("spec").list.borrow().len() > 1);
        self.host.add("specSelect", spec_select.clone());
        *self.spec_select.borrow_mut() = Some(spec_select.clone());
        let pr: ControlRef = spec_select;
        let t1 = t.clone();
        self.host.add(
            "specButton",
            ButtonControl::new(
                anchor("LEFT", &pr, "RIGHT"),
                rect(4.0, 0.0, 90.0, 20.0),
                "Manage...".to_string(),
                move |g| {
                    let _ = t1();
                    super::tree::open_spec_manage_popup(g);
                },
            ),
        );
        self.host.add(
            "specLabel",
            LabelControl::new(
                anchor("RIGHT", &pr, "LEFT"),
                rect(-2.0, 0.0, 0.0, 16.0),
                "^7Passive tree:".to_string(),
            ),
        );
        // :220-234 the sockets of the latest tree (`pob_calc::Env::ordered_slots`: the spec's tree holds them)
        let socket_ids: Vec<u32> = {
            let latest = &pob_data::tree::tree_versions().expect("GameVersions").latest_tree_version;
            let tree = pob_tree::tree::load_passive_tree(latest).unwrap_or_else(|e| panic!("{}", e.0));
            let mut ids: Vec<u32> = tree.sockets.keys().copied().collect();
            ids.sort_unstable();
            ids
        };
        let mut prev2 = pr.clone();
        for id in socket_ids {
            let s = ItemSlotControl::new(g, &prev2, me.clone(), format!("Jewel {id}"), Some("Socket"), Some(id));
            self.sockets.borrow_mut().insert(id, s.clone());
            prev2 = s.clone();
            self.slots.borrow_mut().insert(s.slot_name.clone(), s.clone());
            self.host.add(&format!("slot:{}", s.slot_name), s);
        }
        self.host.add(
            "slotHeader",
            LabelControl::new(
                anchor("BOTTOMLEFT", &sa, "TOPLEFT"),
                rect(0.0, -4.0, 0.0, 16.0),
                "^7Equipped items:".to_string(),
            ),
        );
        // :236-276 the weapon swap
        let swap = |g: &mut Gfx, x: f64, label: &str, to_second: bool| {
            let _ = g;
            let t1 = t.clone();
            let b = ButtonControl::new(
                anchor("BOTTOMRIGHT", &sa, "TOPRIGHT"),
                rect(x, -2.0, 18.0, 18.0),
                label.to_string(),
                move |_g| {
                    let s = t1();
                    if s.read(|e| e.active_item_set().use_second_weapon_set) != to_second {
                        s.edit(|e| {
                            e.set_use_second_weapon_set(to_second);
                            Ok(())
                        });
                        s.add_undo_state(false);
                        s.build().build_flag.set(true);
                    }
                },
            );
            b.over_size_text.set(Some(3.0));
            let t1 = t.clone();
            b.locked
                .set_fn(move |_| t1().read(|e| e.active_item_set().use_second_weapon_set) == to_second);
            b
        };
        let s1 = swap(g, -20.0, "I", false);
        let s2 = swap(g, 0.0, "II", true);
        let s1r: ControlRef = s1.clone();
        self.host.add("weaponSwap1", s1);
        self.host.add("weaponSwap2", s2);
        self.host.add(
            "weaponSwapLabel",
            LabelControl::new(
                anchor("RIGHT", &s1r, "LEFT"),
                rect(-4.0, 0.0, 0.0, 14.0),
                "^7Weapon Set:".to_string(),
            ),
        );

        // :278-283 the list of the build's items
        let smr: ControlRef = set_manage.clone();
        let item_list = ListControl::new(
            anchor("TOPLEFT", &smr, "TOPRIGHT"),
            rect(20.0, 20.0, 360.0, 308.0),
            16.0,
            Some("VERTICAL"),
            true,
            Vec::new(),
            true,
            Box::new(ItemListHooks(me.clone())),
        );
        *item_list.default_text.borrow_mut() = Some("^x7F7F7FThis is the list of items that have been added to this build.\nYou can add items to this list by dragging them from\none of the other lists, or by clicking 'Add to build' when\nviewing an item.".to_string());
        let ilr: ControlRef = item_list.clone();
        let t1 = t.clone();
        let filter = DropDownControl::new(
            g,
            anchor("BOTTOMLEFT", &ilr, "TOPLEFT"),
            rect(0.0, -2.0, 110.0, 18.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| t1().update_item_list(g))),
            None,
            false,
        );
        filter.enable_dropped_width.set(true);
        let fr: ControlRef = filter.clone();
        let t1 = t.clone();
        let sort = ButtonControl::new(
            anchor("LEFT", &fr, "RIGHT"),
            rect(4.0, 0.0, 42.0, 18.0),
            "Sort".to_string(),
            move |g| {
                let s = t1();
                s.sort_item_list();
                s.update_item_list(g);
            },
        );
        let sortr: ControlRef = sort.clone();
        let t1 = t.clone();
        let del_unused = ButtonControl::new(
            anchor("LEFT", &sortr, "RIGHT"),
            rect(4.0, 0.0, 84.0, 18.0),
            "Del Unused".to_string(),
            move |g| {
                let s = t1();
                let order = s.read(Engine::item_order);
                let del: Vec<f64> = order
                    .into_iter()
                    .filter(|id| {
                        s.equipped_slot(*id).is_none()
                            && s.find_equipped_abyss(*id, false).is_none()
                            && s.find_socketed_jewel(*id, false).is_none()
                    })
                    .collect();
                for id in del.iter().rev() {
                    s.delete_item(g, *id, true);
                }
                s.populate_slots(g);
                s.add_undo_state(false);
                s.build().build_flag.set(true);
                s.update_item_list(g);
            },
        );
        let w = Rc::downgrade(&item_list);
        del_unused
            .base
            .enabled
            .set_fn(move |_| !w.upgrade().expect("list").list.borrow().is_empty());
        let dur: ControlRef = del_unused.clone();
        let t1 = t.clone();
        let del_all = ButtonControl::new(
            anchor("LEFT", &dur, "RIGHT"),
            rect(4.0, 0.0, 58.0, 18.0),
            "Del All".to_string(),
            move |g| {
                let t2 = t1.clone();
                main().open_confirm_popup(
                    g,
                    "Delete All",
                    "Are you sure you want to delete all items in this build?",
                    "Delete",
                    Rc::new(move |g: &mut Gfx| {
                        let s = t2();
                        for slot in s.slots.borrow().values() {
                            slot.sel_item_id.set(0.0);
                        }
                        s.edit(Engine::delete_all_items);
                        s.populate_slots(g);
                        s.add_undo_state(false);
                        s.build().build_flag.set(true);
                        let l = s.item_list.borrow().clone().expect("itemList");
                        l.sel_index.set(None);
                        *l.sel_value.borrow_mut() = None;
                        s.update_item_list(g);
                    }),
                    None,
                );
            },
        );
        let w = Rc::downgrade(&item_list);
        del_all
            .base
            .enabled
            .set_fn(move |_| !w.upgrade().expect("list").list.borrow().is_empty());
        let dar: ControlRef = del_all.clone();
        let w = Rc::downgrade(&item_list);
        let delete = ButtonControl::new(
            anchor("LEFT", &dar, "RIGHT"),
            rect(4.0, 0.0, 50.0, 18.0),
            "Delete".to_string(),
            move |g| {
                let l = w.upgrade().expect("list");
                let v = l.sel_value.borrow().clone();
                if let (Some(i), Some(v)) = (l.sel_index.get(), v) {
                    l.hooks.on_sel_delete(&l, g, i, &v);
                }
            },
        );
        let w = Rc::downgrade(&item_list);
        delete
            .base
            .enabled
            .set_fn(move |_| w.upgrade().expect("list").sel_value.borrow().is_some());
        item_list.host.add("loadoutFilter", filter.clone());
        item_list.host.add("sort", sort);
        item_list.host.add("deleteUnused", del_unused);
        item_list.host.add("deleteAll", del_all);
        item_list.host.add("delete", delete);
        self.host.add("itemList", item_list.clone());
        *self.loadout_filter.borrow_mut() = Some(filter);
        *self.item_list.borrow_mut() = Some(item_list.clone());

        // :285-308 the databases
        let label = LabelControl::new(
            anchor("TOPLEFT", &ilr, "BOTTOMLEFT"),
            rect(0.0, 14.0, 0.0, 16.0),
            "^7Import from:".to_string(),
        );
        let t1 = t.clone();
        label.base.shown.set_fn(move |g| t1().anchor.base.height.get(g) < 980.0);
        let lr: ControlRef = label.clone();
        let select_db = DropDownControl::new(
            g,
            anchor("LEFT", &lr, "RIGHT"),
            rect(4.0, 0.0, 150.0, 18.0),
            vec![DropItem::str("Uniques"), DropItem::str("Rare Templates")],
            None,
            None,
            false,
        );
        self.host.add("selectDBLabel", label.clone());
        self.host.add("selectDB", select_db.clone());
        let db_rect = |limit: f64, t: &dyn Fn() -> Rc<ItemsTab>| -> crate::control::RectProps {
            let _ = t;
            [0.0.into(), 76.0.into(), 360.0.into(), limit.into()]
        };
        let unique_db = ItemDb::new(g, anchor("TOPLEFT", &ilr, "BOTTOMLEFT"), db_rect(244.0, &t), me.clone(), "UNIQUE");
        let rare_db = ItemDb::new(g, anchor("TOPLEFT", &ilr, "BOTTOMLEFT"), db_rect(260.0, &t), me.clone(), "RARE");
        for (db, limit) in [(&unique_db, 244.0), (&rare_db, 260.0)] {
            let t1 = t.clone();
            let w = Rc::downgrade(&db.list);
            db.list.base.height.set_fn(move |g| {
                let c = w.upgrade().expect("db list");
                f64::min(limit, t1().max_y.get() - c.get_pos(g).1)
            });
        }
        let (l1, d1) = (Rc::downgrade(&label), Rc::downgrade(&select_db));
        unique_db.list.base.y.set_fn({
            let l1 = l1.clone();
            move |g| if l1.upgrade().expect("label").is_shown(g) { 118.0 } else { 96.0 }
        });
        unique_db.list.base.shown.set_fn({
            let (l1, d1) = (l1.clone(), d1.clone());
            move |g| !l1.upgrade().expect("label").is_shown(g) || d1.upgrade().expect("db").sel_index.get() == 1
        });
        rare_db.list.base.y.set_fn({
            let l1 = l1.clone();
            move |g| if l1.upgrade().expect("label").is_shown(g) { 78.0 } else { 396.0 }
        });
        rare_db
            .list
            .base
            .shown
            .set_fn(move |g| !l1.upgrade().expect("label").is_shown(g) || d1.upgrade().expect("db").sel_index.get() == 2);
        self.host.add("uniqueDB", unique_db.list.clone());
        self.host.add("rareDB", rare_db.list.clone());
        *self.select_db_label.borrow_mut() = Some(label);
        *self.select_db.borrow_mut() = Some(select_db);
        *self.unique_db.borrow_mut() = Some(unique_db.clone());
        *self.rare_db.borrow_mut() = Some(rare_db.clone());

        // :310-332 create / import
        let t1 = t.clone();
        let craft = ButtonControl::new(
            anchor("TOPLEFT", &ilr, "TOPRIGHT"),
            rect(20.0, -20.0, 120.0, 20.0),
            "Craft item...".to_string(),
            move |g| {
                t1().craft_item_popup(g);
            },
        );
        let t1 = t.clone();
        craft.base.shown.set_fn(move |_| t1().display_item.borrow().is_none());
        let cr: ControlRef = craft.clone();
        let t1 = t.clone();
        self.host.add(
            "newDisplayItem",
            ButtonControl::new(
                anchor("TOPLEFT", &cr, "TOPRIGHT"),
                rect(8.0, 0.0, 120.0, 20.0),
                "Create custom...".to_string(),
                move |g| {
                    t1().edit_display_item_text(g, false);
                },
            ),
        );
        self.host.add(
            "displayItemTip",
            LabelControl::new(
                anchor("TOPLEFT", &cr, "BOTTOMLEFT"),
                rect(0.0, 8.0, 100.0, 16.0),
                "^7Double-click an item from one of the lists,\nor copy and paste an item from in game\n(hover over the item and Ctrl+C) to view or edit\nthe item and add it to your build. You can\nalso clone an item within Path of Building by\ncopying and pasting it with Ctrl+C and Ctrl+V.\n\nYou can Control + Click an item to equip it, or\ndrag it onto the slot.  This will also add it to\nyour build if it's from the unique/template list.\nIf there's 2 slots an item can go in,\nholding Shift will put it in the second.".to_string(),
            ),
        );
        let shared = ListControl::new(
            anchor("TOPLEFT", &cr, "BOTTOMLEFT"),
            rect(0.0, 232.0, 340.0, 308.0),
            16.0,
            Some("VERTICAL"),
            true,
            main()
                .shared_item_list
                .borrow()
                .iter()
                .map(|raw| ItemRow::Obj(Rc::new(Item::parse_with(raw, None, false))))
                .collect(),
            true,
            Box::new(SharedListHooks(me.clone())),
        );
        shared.label.set(Some("^7Shared items:".to_string()));
        *shared.default_text.borrow_mut() = Some("^x7F7F7FThis is a list of items that will be shared between all of\nyour builds.\nYou can add items to this list by dragging them from\none of the other lists.".to_string());
        let shr: ControlRef = shared.clone();
        let w = Rc::downgrade(&shared);
        let del = ButtonControl::new(
            anchor("BOTTOMRIGHT", &shr, "TOPRIGHT"),
            rect(0.0, -2.0, 60.0, 18.0),
            "Delete".to_string(),
            move |g| {
                let l = w.upgrade().expect("list");
                let v = l.sel_value.borrow().clone();
                if let (Some(i), Some(v)) = (l.sel_index.get(), v) {
                    l.hooks.on_sel_delete(&l, g, i, &v);
                }
            },
        );
        let w = Rc::downgrade(&shared);
        del.base
            .enabled
            .set_fn(move |_| w.upgrade().expect("list").sel_value.borrow().is_some());
        shared.host.add("delete", del);
        self.host.add("craftDisplayItem", craft.clone());
        self.host.add("sharedItemList", shared.clone());
        *self.craft_display_item.borrow_mut() = Some(craft);
        *self.shared_list.borrow_mut() = Some(shared.clone());

        // :334 the display item
        let ad = BareControl::new(anchor("TOPLEFT", &ilr, "TOPRIGHT"), rect(20.0, -20.0, 0.0, 0.0));
        let t1 = t.clone();
        ad.base.shown.set_fn(move |_| t1().display_item.borrow().is_some());
        self.host.add("anchorDisplayItem", ad.clone());
        *self.anchor_display_item.borrow_mut() = Some(ad.clone());
        let panel = super::item_editor::Panel::new(g, self, &ad);
        *self.panel.borrow_mut() = Some(panel);

        // :1216-1217
        let sbh = ScrollBarControl::new(None, rect(0.0, 0.0, 0.0, 18.0), Some(100.0), Some("HORIZONTAL"), true);
        let sbv = ScrollBarControl::new(None, rect(0.0, 0.0, 18.0, 0.0), Some(100.0), Some("VERTICAL"), true);
        self.host.add("scrollBarH", sbh.clone());
        self.host.add("scrollBarV", sbv.clone());
        *self.scroll_bar_h.borrow_mut() = Some(sbh);
        *self.scroll_bar_v.borrow_mut() = Some(sbv);

        // :1219-1235 the drag targets
        let slot_targets = || -> Vec<Rc<dyn DragTarget<ItemRow>>> {
            self.slots
                .borrow()
                .values()
                .map(|s| Rc::new(SlotTarget(Rc::downgrade(s))) as Rc<dyn DragTarget<ItemRow>>)
                .collect()
        };
        let list_target = |l: &Rc<ListControl<ItemRow>>| Rc::new(SelfTarget(Rc::downgrade(l))) as Rc<dyn DragTarget<ItemRow>>;
        note_not_ported_once_minion();
        let mut targets = vec![list_target(&shared)];
        targets.extend(slot_targets());
        item_list.set_drag_target_list(targets);
        for db in [&unique_db, &rare_db] {
            let mut targets = vec![list_target(&item_list), list_target(&shared)];
            targets.extend(slot_targets());
            db.list.set_drag_target_list(targets);
        }
        let mut targets = vec![list_target(&item_list)];
        targets.extend(slot_targets());
        shared.set_drag_target_list(targets);
    }

    /// `Load`'s end (:1369-1378) + the state of the constructor: the engine loaded the items already.
    fn after_load(&self, g: &mut Gfx) {
        self.sync_gear_slots();
        // :1212 of the constructor; the sockets stay active until the first `UpdateSockets` (:1551, :4911)
        *self.last_slot.borrow_mut() = Some(self.slot(BASE_SLOTS[BASE_SLOTS.len() - 1]));
        self.populate_slots(g);
        self.sync_item_rows();
        self.spec_select
            .borrow()
            .as_ref()
            .expect("specSelect")
            .sel_index
            .set(self.read(Engine::active_spec));
        let first = Rc::new(self.read(Engine::items_load_undo_state));
        self.undo.undo.borrow_mut().clear();
        self.undo.redo.borrow_mut().clear();
        self.undo.undo.borrow_mut().push(first);
        self.undo.mod_flag.set(false);
    }

    /// `self.controls.<name>`'s position, for the gate's cursor.
    pub fn debug_control_pos(&self, g: &Gfx, name: &str, dx: f64, dy: f64) -> (f64, f64) {
        let c = self.debug_control(name).unwrap_or_else(|| panic!("control {name}"));
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        ((x + w / 2.0).floor() + dx, (y + h / 2.0).floor() + dy)
    }

    pub fn debug_child(&self, parent: &str, child: &str) -> Option<ControlRef> {
        if let Some(n) = parent.strip_prefix("slot:") {
            return self.slots.borrow().get(n)?.drop.host.get(child);
        }
        match parent {
            "itemList" => self.item_list.borrow().as_ref()?.host.get(child),
            "sharedItemList" => self.shared_list.borrow().as_ref()?.host.get(child),
            "uniqueDB" => self.unique_db.borrow().as_ref()?.list.host.get(child),
            "rareDB" => self.rare_db.borrow().as_ref()?.list.host.get(child),
            _ => None,
        }
    }

    pub fn debug_control(&self, name: &str) -> Option<ControlRef> {
        if let Some(n) = name.strip_prefix("slot:") {
            return self.slots.borrow().get(n).map(|s| s.clone() as ControlRef);
        }
        if let Some(c) = self.host.get(name) {
            return Some(c);
        }
        self.panel.borrow().as_ref().and_then(|p| p.host_control(name))
    }

    pub(crate) fn snap(&self, side: Option<&'static str>) {
        self.snap_h.set(side);
    }

    /// The gate's state (the same fields as `items-runner.lua`).
    #[allow(clippy::too_many_lines)]
    pub fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        use serde_json::json;
        let drop = |x: &DropDownControl| {
            let list: Vec<String> = x.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect();
            let shown = x.is_shown(g);
            json!({ "shown": shown, "enabled": shown && x.is_enabled(g), "sel": x.sel_index.get(), "list": list })
        };
        let tip = |t: &Tooltip| {
            t.lines
                .iter()
                .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
                .collect::<Vec<_>>()
        };
        let il = self.item_list.borrow().clone().expect("itemList");
        let rows: Vec<String> = il
            .list
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, r)| il.hooks.get_row_value(&il, g, 1, i + 1, r))
            .collect();
        let mut slots = serde_json::Map::new();
        for s in self.slots.borrow().values() {
            if s.is_shown(g) {
                let (x, y) = s.get_pos(g);
                slots.insert(
                    s.slot_name.clone(),
                    json!({ "label": s.label.borrow().clone(), "sel": s.sel_item_id.get(), "pos": format!("{x},{y}"), "active": s.active.get() == Some(true), "drop": drop(&s.drop) }),
                );
            }
        }
        let sets: Vec<_> = self.read(|e| {
            e.item_sets()
                .iter()
                .map(|s| json!({ "id": s.id, "title": s.title, "second": s.use_second_weapon_set }))
                .collect()
        });
        let order = self.read(Engine::item_order);
        let items: Vec<String> = self.read(|e| {
            order
                .iter()
                .filter_map(|id| e.item_by_id(*id).map(|i| format!("{id}|{}", i.build_raw())))
                .collect()
        });
        let mut d = json!({
            "rows": rows, "order": order, "items": items, "slots": slots, "sets": sets,
            "active": self.read(Engine::active_item_set_id),
            "setSelect": drop(&self.set_select),
            "specSelect": drop(&self.spec_select.borrow().clone().expect("specSelect")),
            "listSel": il.sel_index.get(), "listTip": tip(&il.tooltip.borrow()),
            "filter": drop(&self.loadout_filter.borrow().clone().expect("filter")),
            "undo": self.undo.undo.borrow().len(), "redo": self.undo.redo.borrow().len(),
            "showStat": self.read(Engine::items_show_stat_differences),
            "scrollH": self.scroll_bar_h.borrow().as_ref().expect("h").offset.get(),
            "scrollV": self.scroll_bar_v.borrow().as_ref().expect("v").offset.get(),
            "selectDB": self.select_db.borrow().as_ref().expect("db").sel_index.get(),
        });
        for (key, db) in [("uniqueDB", &self.unique_db), ("rareDB", &self.rare_db)] {
            let db = db.borrow().clone().expect("db");
            if db.list.is_shown(g) {
                let names: Vec<String> = db
                    .list
                    .list
                    .borrow()
                    .iter()
                    .enumerate()
                    .map(|(i, r)| db.list.hooks.get_row_value(&db.list, g, 1, i + 1, r))
                    .collect();
                d[key] = json!({ "n": names.len(), "first": names, "tip": tip(&db.list.tooltip.borrow()),
                    "slot": drop(&db.slot), "type": drop(&db.ty), "search": db.search.buf.borrow().clone(), "mode": db.search_mode.sel_index.get() });
            }
        }
        for s in self.slots.borrow().values() {
            if s.is_shown(g) && !s.drop.tip.tooltip.borrow().lines.is_empty() {
                d["slotTip"] = json!(tip(&s.drop.tip.tooltip.borrow()));
            }
        }
        d["display"] = self.panel.borrow().as_ref().expect("panel").debug_dump(g, self);
        if let Some(p) = main().popups.borrow().first() {
            let mut q = serde_json::Map::new();
            let names = p.control_names();
            for n in [
                "rarity",
                "type",
                "base",
                "source",
                "sort",
                "enchantment",
                "enchantmentSource",
                "skill",
                "modSelect",
                "implicit1",
                "implicit2",
                "edit",
                "title",
                "custom",
                "save",
            ] {
                if !names.iter().any(|x| x == n) {
                    continue;
                }
                let c = p.control(n);
                let any: &dyn std::any::Any = &*c;
                if let Some(x) = any.downcast_ref::<DropDownControl>() {
                    q.insert(n.to_string(), drop(x));
                } else if let Some(x) = any.downcast_ref::<EditControl>() {
                    q.insert(n.to_string(), json!(x.buf.borrow().clone()));
                } else if let Some(x) = any.downcast_ref::<ButtonControl>() {
                    q.insert("saveLabel".to_string(), json!(x.label.get(g)));
                    q.insert("saveTip".to_string(), json!(tip(&x.tip.tooltip.borrow())));
                    q.insert("saveEnabled".to_string(), json!(x.is_enabled(g)));
                }
            }
            if names.iter().any(|x| x == "searchResults")
                && let Some(l) = super::timeless::debug_results_tooltip(&p.control("searchResults"))
            {
                q.insert("timelessTip".to_string(), json!(l));
            }
            if names.iter().any(|x| x == "notableDB") {
                let c = p.control("notableDB");
                let any: &dyn std::any::Any = &*c;
                if let Some(l) = any.downcast_ref::<ListControl<super::item_popups::NotableRow>>() {
                    let rows: Vec<String> = l
                        .list
                        .borrow()
                        .iter()
                        .enumerate()
                        .map(|(i, r)| l.hooks.get_row_value(l, g, 1, i + 1, r))
                        .collect();
                    q.insert("notables".to_string(), json!(rows));
                    q.insert("notableTip".to_string(), json!(tip(&l.tooltip.borrow())));
                }
            }
            d["popup"] = serde_json::Value::Object(q);
        }
        if let Some(s) = self.host.sel() {
            d["sel"] = json!(self.debug_name_of(&s));
        }
        d
    }

    fn debug_name_of(&self, c: &ControlRef) -> String {
        for (name, x) in self.host.controls_named() {
            if crate::control::same(&*x, &**c) {
                // `self.DropDownControl:OnKeyDown(key)` returns the class proxy, not the slot (ItemSlotControl.lua:171)
                return if name.starts_with("slot:") { "other".to_string() } else { name };
            }
        }
        "other".to_string()
    }

    /// `gemForSkill` lookup kept for the tooltip of the granted skill.
    pub fn gem_for(&self, id: &str) -> bool {
        gem_of(id).is_some()
    }
}

/// :1221-1229 `build.controls.mainSkillMinion` in the drag targets, Build.lua:541-550.
fn note_not_ported_once_minion() {
    note_not_ported("Classes/ItemsTab.lua:1221-1229, Modules/Build.lua:541-550: dragging an item onto mainSkillMinion (EquipItemInSet)");
}

/// `note_not_ported` re-exported for `item_editor.rs`.
pub(crate) fn note(what: &str) {
    note_not_ported(what);
}

/// `tools/sg-oracle/items-rust-input.ps1`: the tab's state after each frame, for posted window messages (`runner:RealState` of items-runner.lua).
pub fn debug_input_state(g: &Gfx) -> serde_json::Value {
    use serde_json::json;
    let b = main().build_mode.clone();
    if main().mode.borrow().as_deref() != Some("BUILD") {
        return serde_json::Value::Null;
    }
    let Some(t) = b.items_tab.borrow().clone() else {
        return serde_json::Value::Null;
    };
    let pos = |c: &dyn Control| {
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        json!({ "x": (x + w / 2.0).floor(), "y": (y + h / 2.0).floor() })
    };
    let list = t.item_list.borrow().clone().expect("itemList");
    let (lx, ly) = list.get_pos(g);
    let mode = b.host.get("modeItems").expect("modeItems");
    let db = t.unique_db.borrow().clone().expect("uniqueDB");
    let craft = t.craft_display_item.borrow().clone().expect("craft");
    let popup_save = main().popups.borrow().first().and_then(|p| p.host.get("save")).map(|c| pos(&*c));
    json!({
        "viewMode": *b.view_mode.borrow(), "modeBtn": pos(&*mode), "rowPos": { "x": lx + 10.0, "y": ly + 2.0 + 16.0 + 8.0 },
        "listSel": list.sel_index.get().unwrap_or(0),
        "displayRaw": t.display_item.borrow().as_ref().map_or_else(|| "nil".to_string(), |i| i.build_raw()),
        "craftPos": pos(&*craft), "popupSave": popup_save, "searchPos": pos(&*db.search), "search": db.search.buf.borrow().clone(), "dbN": db.list.list.borrow().len(),
        "scrollV": t.scroll_bar_v.borrow().as_ref().expect("v").offset.get(),
    })
}
