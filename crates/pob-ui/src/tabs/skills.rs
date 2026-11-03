//! `Classes/SkillsTab.lua`, `Classes/SkillListControl.lua`, `Classes/SkillSetListControl.lua`. The socket groups live in the engine (`Engine::skills_tab`), this module keeps
//! the controls and a token per row: upstream's list holds the group tables themselves, compared by identity.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_engine::engine::Engine;
use pob_engine::skills_tab::{GemEdit, SkillsUndoState};
use pob_gfx::{Frame, Gfx, ImageHandle};

use super::gem_select::{GemChangeFunc, GemSelectControl, gem_of};
use crate::colors::{color_code, hex_to_rgb};
use crate::control::{Control, ControlRef, Prop, Rect, add_to_tab_group, anchor, rect, same};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl, SectionControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{ByRef, ListControl, ListHooks};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::gem_tooltip::{add_socket_group_tooltip, skill_color};
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::lua::tostring_num;
use crate::modules::build::BuildMode;
use crate::modules::main::{BareControl, main, sel_by_value};
use crate::sg;
use crate::tooltip::Tooltip;
use crate::undo::{UndoHandler, UndoHost};

/// A row of the group list: the identity of a `socketGroup` table.
pub type GroupRef = ByRef<()>;

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

fn token() -> GroupRef {
    ByRef(Rc::new(()))
}

fn item<T: 'static>(label: &str, data: T) -> DropItem {
    let mut it = DropItem::table(label);
    it.data = Some(Rc::new(data) as Rc<dyn Any>);
    it
}

fn data_of<T: Clone + 'static>(it: &DropItem) -> T {
    it.data
        .as_ref()
        .and_then(|d| d.downcast_ref::<T>())
        .cloned()
        .expect("drop-down item data")
}

/// `groupSlotDropList` (:13-28).
fn group_slot_list() -> Vec<DropItem> {
    let mut l = vec![item("None", None::<String>)];
    for (label, slot) in [
        ("Weapon 1", "Weapon 1"),
        ("Weapon 2", "Weapon 2"),
        ("Weapon 1 (Swap)", "Weapon 1 Swap"),
        ("Weapon 2 (Swap)", "Weapon 2 Swap"),
        ("Helmet", "Helmet"),
        ("Body Armour", "Body Armour"),
        ("Gloves", "Gloves"),
        ("Boots", "Boots"),
        ("Amulet", "Amulet"),
        ("Ring 1", "Ring 1"),
        ("Ring 2", "Ring 2"),
        ("Ring 3", "Ring 3"),
        ("Belt", "Belt"),
    ] {
        l.push(item(label, Some(slot.to_string())));
    }
    l
}

/// `defaultGemLevelList` (:30-58).
const DEFAULT_LEVELS: [(&str, &str, &str); 5] = [
    (
        "Normal Maximum",
        "All gems default to their highest valid non-corrupted gem level.",
        "normalMaximum",
    ),
    (
        "Corrupted Maximum",
        "Normal gems default to their highest valid corrupted gem level.\nAwakened gems default to their highest valid non-corrupted gem level.",
        "corruptedMaximum",
    ),
    (
        "Awakened Maximum",
        "All gems default to their highest valid corrupted gem level.",
        "awakenedMaximum",
    ),
    (
        "Match Character Level",
        "All gems default to their highest valid non-corrupted gem level that your character meets the level requirement for.\nThis hides gems with a minimum level requirement above your character level, preventing them from showing up in the dropdown list.",
        "characterLevel",
    ),
    ("Level 1", "All gems default to level 1.", "levelOne"),
];

const SHOW_SUPPORT: [(&str, &str); 3] = [("All", "ALL"), ("Non-Exceptional", "NORMAL"), ("Exceptional", "EXCEPTIONAL")];

const SORT_TYPES: [(&str, &str); 9] = [
    ("Full DPS", "FullDPS"),
    ("Combined DPS", "CombinedDPS"),
    ("Hit DPS", "TotalDPS"),
    ("Average Hit", "AverageDamage"),
    ("DoT DPS", "TotalDot"),
    ("Bleed DPS", "BleedDPS"),
    ("Ignite DPS", "IgniteDPS"),
    ("Poison DPS", "TotalPoisonDPS"),
    ("Effective Hit Pool", "TotalEHP"),
];

const GROUP_TIP: &str = "^7Usage Tips:
- You can copy/paste socket groups using Ctrl+C and Ctrl+V.
- Ctrl + Click to enable/disable socket groups.
- Ctrl + Right click to include/exclude in FullDPS calculations.
- Right click to set as the Main skill group.
";

/// `slot_map` (SkillListControl.lua:9-27).
const SLOT_ICONS: [(&str, &str); 17] = [
    ("Weapon 1", "Assets/icon_weapon.png"),
    ("Weapon 2", "Assets/icon_weapon_2.png"),
    ("Weapon 1 Swap", "Assets/icon_weapon_swap.png"),
    ("Weapon 2 Swap", "Assets/icon_weapon_2_swap.png"),
    ("Bow", "Assets/icon_bow.png"),
    ("Quiver", "Assets/icon_quiver.png"),
    ("Shield", "Assets/icon_shield.png"),
    ("Shield Swap", "Assets/icon_shield_swap.png"),
    ("Helmet", "Assets/icon_helmet.png"),
    ("Body Armour", "Assets/icon_body_armour.png"),
    ("Gloves", "Assets/icon_gloves.png"),
    ("Boots", "Assets/icon_boots.png"),
    ("Amulet", "Assets/icon_amulet.png"),
    ("Ring 1", "Assets/icon_ring_left.png"),
    ("Ring 2", "Assets/icon_ring_right.png"),
    ("Ring 3", "Assets/icon_ring_right.png"),
    ("Belt", "Assets/icon_belt.png"),
];

/// `self.gemSlots[index]`.
pub struct GemSlot {
    pub delete: Rc<ButtonControl>,
    pub name_spec: Rc<GemSelectControl>,
    pub level: Rc<EditControl>,
    pub quality: Rc<EditControl>,
    pub enabled: Rc<CheckBoxControl>,
    pub count: Rc<EditControl>,
    pub err_msg: Rc<LabelControl>,
    pub enable_global1: Rc<CheckBoxControl>,
    pub enable_global2: Rc<CheckBoxControl>,
}

pub struct SkillsTab {
    pub host: ControlHost,
    me: Weak<Self>,
    build: Weak<BuildMode>,
    pub undo: UndoHandler<Rc<SkillsUndoState>>,
    /// `self` as an anchor.
    anchor: Rc<BareControl>,
    pub set_select: Rc<DropDownControl>,
    pub group_list: Rc<ListControl<GroupRef>>,
    pub sort_gems_by_dps: Rc<CheckBoxControl>,
    pub sort_field: Rc<DropDownControl>,
    pub default_level: Rc<DropDownControl>,
    pub default_quality: Rc<EditControl>,
    pub show_support: Rc<DropDownControl>,
    pub show_legacy: Rc<CheckBoxControl>,
    pub group_label: Rc<EditControl>,
    pub group_slot: Rc<DropDownControl>,
    pub group_enabled: Rc<CheckBoxControl>,
    pub include_in_full_dps: Rc<CheckBoxControl>,
    pub sockets_label: Rc<LabelControl>,
    pub optimise_sockets: Rc<ButtonControl>,
    pub imbued_label: Rc<LabelControl>,
    pub imbued_support: RefCell<Option<Rc<GemSelectControl>>>,
    pub imbued_clear: Rc<ButtonControl>,
    pub group_count: Rc<EditControl>,
    pub group_count_label: Rc<LabelControl>,
    pub source_note: Rc<LabelControl>,
    pub scroll_bar_h: Rc<ScrollBarControl>,
    anchor_group_detail: Rc<BareControl>,
    anchor_gem_slots: Rc<BareControl>,
    pub gem_slots: RefCell<Vec<Rc<GemSlot>>>,
    /// `self.displayGroup`.
    pub display_group: RefCell<Option<GroupRef>>,
    /// What the list's `New` button returns: the tab's next `selControl` (a port's button returns nothing; applied right after `ProcessControlsInput`).
    pending_focus: RefCell<Option<ControlRef>>,
    /// The manage popup's list.
    pub set_list: RefCell<Weak<ListControl<f64>>>,
    icons: RefCell<HashMap<&'static str, ImageHandle>>,
}

impl UndoHost for SkillsTab {
    type State = Rc<SkillsUndoState>;
    fn undo_handler(&self) -> &UndoHandler<Self::State> {
        &self.undo
    }
    fn create_undo_state(&self) -> Self::State {
        Rc::new(self.read(Engine::skills_undo_state))
    }
    fn restore_undo_state(&self, _state: Self::State) {
        unreachable!("SkillsTab restores through undo_g / redo_g: its controls need the host");
    }
}

struct SkillListHooks(Weak<SkillsTab>);

impl ListHooks<GroupRef> for SkillListHooks {
    /// SkillListControl.lua:79-118
    fn get_row_value(&self, _l: &ListControl<GroupRef>, _g: &Gfx, column: usize, index: usize, _v: &GroupRef) -> String {
        if column != 1 {
            return String::new();
        }
        let t = self.0.upgrade().expect("skillsTab");
        t.read(|e| {
            let groups = e.socket_groups();
            let Some(g) = groups.get(index - 1) else { return String::new() };
            let mut label = g.display_label.clone().unwrap_or_else(|| "?".to_string());
            let current = e.main_socket_group() == index as f64;
            let disabled = !g.enabled || g.slot_enabled != Some(true);
            if disabled {
                let colour = if current { "" } else { "^x7F7F7F" };
                label = format!("{colour}{label} (Disabled)");
            }
            if current {
                let active = if disabled { " (Forced Active)" } else { " (Active)" };
                label = format!("{label}{}{active}", cc("RELIC"));
            }
            if g.include_in_full_dps == Some(true) {
                label = format!("{label}{} (FullDPS)", cc("CUSTOM"));
            }
            if g.source.is_none() {
                let skills = pob_data::skills::skills();
                let mut s = String::new();
                for gem in &g.gem_list {
                    let ge = gem.granted_effect.as_ref().and_then(|i| skills.get(i)).or_else(|| {
                        gem.gem_data
                            .as_deref()
                            .and_then(gem_of)
                            .and_then(|d| skills.get(&d.granted_effect_id))
                    });
                    if let (Some(ge), true) = (ge, gem.enabled) {
                        let ch = match ge.color {
                            Some(1.0) => "R",
                            Some(2.0) => "G",
                            Some(3.0) => "B",
                            _ => "W",
                        };
                        if !s.is_empty() {
                            s.push_str("^7-");
                        }
                        s.push_str(gem.color.as_deref().unwrap_or(""));
                        s.push_str(ch);
                    }
                }
                if !s.is_empty() {
                    label = format!("{label} ^7{s}");
                }
            }
            label
        })
    }

    /// SkillListControl.lua:241-267
    fn get_row_icon(&self, _l: &ListControl<GroupRef>, column: usize, index: usize, _v: &GroupRef) -> Option<ImageHandle> {
        if column != 1 {
            return None;
        }
        let t = self.0.upgrade().expect("skillsTab");
        t.read(|e| {
            let slot = e.socket_groups().get(index - 1)?.slot.clone()?;
            let base_ty = |s: &str| {
                e.slot_item(s)
                    .and_then(|i| i.base.map(|b| b.ty.clone()))
                    .unwrap_or_else(|| "None".to_string())
            };
            let mut slot = slot;
            if slot == "Weapon 1" && base_ty("Weapon 1") == "Bow" {
                slot = "Bow".to_string();
            }
            if slot == "Weapon 1 Swap" && base_ty("Weapon 1 Swap") == "Bow" {
                slot = "Bow Swap".to_string();
            }
            if slot == "Weapon 2" {
                let w = base_ty("Weapon 2");
                if w == "Quiver" || w == "Shield" {
                    slot = w;
                }
            }
            if slot == "Weapon 2 Swap" {
                let w = base_ty("Weapon 2 Swap");
                if w == "Quiver" || w == "Shield" {
                    slot = format!("{w} Swap");
                }
            }
            t.icon(&slot)
        })
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// SkillListControl.lua:120-128
    fn add_value_tooltip(&self, _l: &ListControl<GroupRef>, g: &Gfx, tooltip: &mut Tooltip, index: usize, v: &GroupRef) {
        let t = self.0.upgrade().expect("skillsTab");
        t.read(|e| {
            if e.socket_groups().get(index - 1).is_none_or(|x| x.display_skill_list.is_none()) {
                tooltip.clear(false);
                return;
            }
            if tooltip.check_for_update((v.clone(), e.output_revision())) {
                add_socket_group_tooltip(tooltip, g, e, index);
            }
        });
    }

    /// SkillListControl.lua:130-149 (the list was reordered by `ListControl`).
    fn on_order_change(&self, _l: &ListControl<GroupRef>, old_index: usize, new_index: usize) {
        let t = self.0.upgrade().expect("skillsTab");
        t.edit(|e| e.move_socket_group(old_index, new_index));
        t.add_undo_state(false);
        t.build().build_flag.set(true);
    }

    /// :151-153
    fn on_select(&self, _l: &ListControl<GroupRef>, g: &mut Gfx, index: usize, _v: &GroupRef) {
        self.0.upgrade().expect("skillsTab").set_display_group(g, Some(index));
    }

    /// :155-159
    fn on_sel_copy(&self, _l: &ListControl<GroupRef>, g: &mut Gfx, index: usize, _v: &GroupRef) {
        let t = self.0.upgrade().expect("skillsTab");
        if t.read(|e| e.socket_groups().get(index - 1).is_some_and(|x| x.source.is_none())) {
            let text = t.read(|e| e.copy_socket_group_text(index)).unwrap_or_else(|er| panic!("{er}"));
            g.copy(&text);
        }
    }

    /// :161-197
    fn on_sel_delete(&self, l: &ListControl<GroupRef>, g: &mut Gfx, index: usize, _v: &GroupRef) {
        let t = self.0.upgrade().expect("skillsTab");
        let (source, empty, label) = t.read(|e| {
            let x = &e.socket_groups()[index - 1];
            (x.source.is_some(), x.gem_list.is_empty(), x.display_label.clone())
        });
        if source {
            main().open_message_popup(
                g,
                "Delete Socket Group",
                "This socket group cannot be deleted as it is created by an equipped item.",
            );
        } else if empty {
            t.delete_group(g, l, index, false);
        } else {
            let label = label.expect("Classes/SkillListControl.lua:185: attempt to concatenate field 'displayLabel' (a nil value)");
            let w = self.0.clone();
            main().open_confirm_popup(
                g,
                "Delete Socket Group",
                &format!("Are you sure you want to delete '{label}'?"),
                "Delete",
                Rc::new(move |g: &mut Gfx| {
                    let t = w.upgrade().expect("skillsTab");
                    let list = t.group_list.clone();
                    t.delete_group(g, &list, index, true);
                }),
                None,
            );
        }
    }

    fn has_on_hover_key_up(&self) -> bool {
        true
    }

    /// :199-234
    fn on_hover_key_up(&self, l: &ListControl<GroupRef>, g: &mut Gfx, key: &str) {
        let t = self.0.upgrade().expect("skillsTab");
        let Some(index) = l.get_hover_index(g) else { return };
        if key == "F1" {
            // :202-208 `itemLib.wiki.openGem`
            let name = t.read(|e| e.socket_groups()[index - 1].gem_list.first().and_then(|g| g.gem_data.clone()));
            if let Some(d) = name.as_deref().and_then(gem_of) {
                let support = pob_data::skills::skills().get(&d.granted_effect_id).is_some_and(|x| x.support);
                let n = if support { format!("{} Support", d.name) } else { d.name.clone() };
                crate::tabs::items::wiki_open(g, &n);
            }
        } else if key == "RIGHTBUTTON" {
            if sg::is_key_down(g, "CTRL") {
                let state = t.read(|e| e.socket_groups()[index - 1].include_in_full_dps != Some(true));
                t.edit(|e| e.set_group_full_dps(index, state));
                if t.display_index() == Some(index) {
                    t.set_display_group(g, Some(index));
                }
            } else {
                t.edit(|e| {
                    e.set_main_group(index);
                    Ok(())
                });
            }
            t.add_undo_state(false);
            t.build().build_flag.set(true);
        } else if key == "LEFTBUTTON" && sg::is_key_down(g, "CTRL") {
            let state = t.read(|e| !e.socket_groups()[index - 1].enabled);
            t.edit(|e| e.set_group_enabled(index, state));
            if t.display_index() == Some(index) {
                t.set_display_group(g, Some(index));
            }
            t.add_undo_state(false);
            t.build().build_flag.set(true);
        }
    }
}

struct SetListHooks(Weak<SkillsTab>);

impl ListHooks<f64> for SetListHooks {
    /// SkillSetListControl.lua:88-93
    fn get_row_value(&self, _l: &ListControl<f64>, _g: &Gfx, column: usize, _index: usize, id: &f64) -> String {
        if column != 1 {
            return String::new();
        }
        let t = self.0.upgrade().expect("skillsTab");
        t.read(|e| {
            let title = e.skill_sets().into_iter().find(|s| s.0 == *id).and_then(|s| s.1);
            let current = if *id == e.skills_tab().active_skill_set_id {
                "  ^9(Current)"
            } else {
                ""
            };
            format!("{}{current}", title.as_deref().unwrap_or("Default"))
        })
    }

    /// :95-97
    fn on_order_change(&self, _l: &ListControl<f64>, old_index: usize, new_index: usize) {
        let t = self.0.upgrade().expect("skillsTab");
        t.edit(|e| e.move_skill_set(old_index, new_index));
        t.undo.mod_flag.set(true);
    }

    /// :99-104
    fn on_sel_click(&self, _l: &ListControl<f64>, g: &mut Gfx, _index: usize, id: &f64, double_click: bool) -> Option<bool> {
        let t = self.0.upgrade().expect("skillsTab");
        if double_click && *id != t.read(|e| e.skills_tab().active_skill_set_id) {
            t.set_active_skill_set(g, *id);
            t.add_undo_state(false);
        }
        None
    }

    /// :106-121
    fn on_sel_delete(&self, l: &ListControl<f64>, g: &mut Gfx, index: usize, id: &f64) {
        if l.list.borrow().len() > 1 {
            self.0.upgrade().expect("skillsTab").confirm_delete_set(g, index, *id);
        }
    }

    /// :123-127
    fn on_sel_key_down(&self, _l: &ListControl<f64>, g: &mut Gfx, _index: usize, id: &f64, key: &str) {
        if key == "F2" {
            self.0.upgrade().expect("skillsTab").rename_set_popup(g, RenameSet::Existing(*id));
        }
    }
}

enum RenameSet {
    Existing(f64),
    New,
    Copy(f64),
}

impl SkillsTab {
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

    fn icon(&self, slot: &str) -> Option<ImageHandle> {
        let (k, path) = SLOT_ICONS.iter().find(|(k, _)| *k == slot)?;
        Some(
            self.icons
                .borrow_mut()
                .entry(k)
                .or_insert_with(|| {
                    let h = ImageHandle::new();
                    let root = crate::launch::launch().cfg.script_path.clone();
                    let _ = h.load(format!("{root}{path}"), &[]);
                    h
                })
                .clone(),
        )
    }

    /// `self.displayGroup`'s position in the list.
    pub fn display_index(&self) -> Option<usize> {
        let d = self.display_group.borrow();
        let d = d.as_ref()?;
        self.group_list.list.borrow().iter().position(|x| x == d).map(|i| i + 1)
    }

    pub fn display_token(&self) -> Option<GroupRef> {
        self.display_group.borrow().clone()
    }

    /// The control as a `GemSelectControl` (`control._className == "GemSelectControl"`).
    pub fn gem_select_of(&self, c: &ControlRef) -> Option<Rc<GemSelectControl>> {
        if let Some(i) = self.imbued_support.borrow().as_ref()
            && same(&**i, &**c)
        {
            return Some(i.clone());
        }
        self.gem_slots
            .borrow()
            .iter()
            .find(|s| same(&*s.name_spec, &**c))
            .map(|s| s.name_spec.clone())
    }

    fn imbued(&self) -> Rc<GemSelectControl> {
        self.imbued_support.borrow().clone().expect("imbuedSupport")
    }

    /// `self.controls.groupList.list` over the engine's list: CalcSetup adds and removes groups at the end.
    fn sync_list(&self) {
        let n = self.read(|e| e.socket_groups().len());
        let mut l = self.group_list.list.borrow_mut();
        l.truncate(n);
        while l.len() < n {
            l.push(token());
        }
    }

    /// The confirmed part of `OnSelDelete` (SkillListControl.lua:175-183, :186-194).
    fn delete_group(&self, g: &mut Gfx, l: &ListControl<GroupRef>, index: usize, confirmed: bool) {
        let removed = l.list.borrow_mut().remove(index - 1);
        self.edit(|e| e.delete_socket_group(index));
        let is_shown = self.display_group.borrow().as_ref() == Some(&removed);
        if is_shown {
            if confirmed {
                self.set_display_group(g, None);
            } else {
                *self.display_group.borrow_mut() = None;
            }
        }
        self.add_undo_state(false);
        self.build().build_flag.set(true);
        *l.sel_value.borrow_mut() = None;
    }

    /// `SetActiveSkillSet` (:1478-1502).
    pub fn set_active_skill_set(&self, g: &mut Gfx, id: f64) {
        self.edit(|e| e.set_active_skill_set(id));
        *self.group_list.list.borrow_mut() = Vec::new();
        self.sync_list();
        self.build().build_flag.set(true);
        let first = (!self.group_list.list.borrow().is_empty()).then_some(1);
        self.set_display_group(g, first);
        self.build().sync_loadouts_pub(g);
    }

    /// `SetDisplayGroup(socketGroup)` (:1275-1308). `ProcessSocketGroup` of an unchanged group changes nothing: the engine's tab went through it.
    pub fn set_display_group(&self, g: &mut Gfx, index: Option<usize>) {
        let token = index.and_then(|i| self.group_list.list.borrow().get(i - 1).cloned());
        *self.display_group.borrow_mut() = token.clone();
        let Some(index) = index.filter(|_| token.is_some()) else { return };
        let group = self.read(|e| e.socket_groups()[index - 1].clone());
        self.group_label.set_text(g, group.label.as_deref().unwrap_or("nil"), false);
        sel_by_value(&self.group_slot, &group.slot);
        self.group_enabled.state.set(group.enabled);
        self.include_in_full_dps
            .state
            .set(group.include_in_full_dps == Some(true) && group.enabled);
        self.group_count.set_text(g, &tostring_num(group.group_count.unwrap_or(1.0)), false);
        let imbued = self.imbued();
        if let Some(name) = &group.imbued_support {
            let d = pob_data::gems::gems();
            let gem = d
                .gem_for_base_name
                .get(&format!("{} support", name.to_ascii_lowercase()))
                .and_then(|i| d.gems.0.get(i))
                .expect("Classes/SkillsTab.lua:1290: attempt to index local 'gemId' (a nil value)");
            *imbued.gem_id.borrow_mut() = Some(gem.id.clone());
            imbued.edit.set_text(g, name, false);
            let skills = pob_data::skills::skills();
            *imbued.edit.inactive_col.borrow_mut() = skill_color(skills.get(&gem.granted_effect_id).and_then(|x| x.color));
        } else {
            *imbued.gem_id.borrow_mut() = None;
            imbued.edit.set_text(g, "", false);
        }
        self.update_gem_slots(g);
        for (i, gem) in group.gem_list.iter().enumerate() {
            let slot = self.gem_slots.borrow()[i].clone();
            slot.name_spec.edit.set_text(g, gem.name_spec.as_deref().unwrap_or(""), false);
            slot.level
                .set_text(g, &gem.level.map_or_else(|| "nil".to_string(), tostring_num), false);
            slot.quality
                .set_text(g, &gem.quality.map_or_else(|| "nil".to_string(), tostring_num), false);
            slot.enabled.state.set(gem.enabled);
            slot.enable_global1.state.set(gem.enable_global1 == Some(true));
            slot.enable_global2.state.set(gem.enable_global2 == Some(true));
            slot.count.set_text(g, &tostring_num(gem.count.unwrap_or(1.0)), false);
        }
    }

    /// `UpdateGemSlots` (:1081-1100).
    fn update_gem_slots(&self, g: &mut Gfx) {
        let Some(group) = self.display_index() else { return };
        let colors: Vec<Option<String>> = self.read(|e| e.socket_groups()[group - 1].gem_list.iter().map(|x| x.color.clone()).collect());
        for i in 1..=colors.len() + 1 {
            if self.gem_slots.borrow().len() < i {
                self.create_gem_slot(g, i);
            }
            let slot = self.gem_slots.borrow()[i - 1].clone();
            if i == colors.len() + 1 {
                slot.name_spec.edit.set_text(g, "", false);
                slot.level.set_text(g, "", false);
                slot.quality.set_text(g, "", false);
                slot.enabled.state.set(false);
                slot.count.set_text(g, "1", false);
            } else {
                *slot.name_spec.edit.inactive_col.borrow_mut() = colors[i - 1].clone().unwrap_or_default();
            }
        }
    }

    /// `deleteGem` (:747-763).
    fn delete_gem(&self, g: &mut Gfx, index: usize) {
        let group = self.display_index().expect("displayGroup");
        self.edit(|e| e.delete_gem(group, index));
        let gems = self.read(|e| e.socket_groups()[group - 1].gem_list.clone());
        for (k, gem) in gems.iter().enumerate().skip(index - 1) {
            let slot = self.gem_slots.borrow()[k].clone();
            slot.name_spec.edit.set_text(g, gem.name_spec.as_deref().unwrap_or(""), false);
            slot.level
                .set_text(g, &gem.level.map_or_else(|| "nil".to_string(), tostring_num), false);
            slot.quality
                .set_text(g, &gem.quality.map_or_else(|| "nil".to_string(), tostring_num), false);
            slot.enabled.state.set(gem.enabled);
            slot.enable_global1.state.set(gem.enable_global1 == Some(true));
            slot.enable_global2.state.set(gem.enable_global2 == Some(true));
            slot.count.set_text(g, &tostring_num(gem.count.unwrap_or(1.0)), false);
        }
        self.add_undo_state(false);
        self.build().build_flag.set(true);
    }

    /// `gemList[index]` of a field callback's empty slot (:845-850, :867-872, :958-962, :997-1002).
    fn ensure_gem(&self, g: &mut Gfx, slot: &GemSlot, group: usize, index: usize, level_text: bool, quality_text: bool, count_text: bool) {
        if self.read(|e| e.socket_groups()[group - 1].gem_list.len()) < index {
            self.edit(|e| e.add_gem_from_field(group));
            let q = self.read(|e| e.skills_tab().default_gem_quality);
            if level_text {
                // `slot.level:SetText(gemInstance.level)`: `self.defaultGemLevel or 20`, the option's name
                let l = self.read(|e| e.skills_tab().default_gem_level.clone());
                slot.level.set_text(g, &l, false);
            }
            if quality_text {
                slot.quality.set_text(g, &tostring_num(q), false);
            }
            slot.enabled.state.set(true);
            slot.enable_global1.state.set(true);
            if count_text {
                slot.count.set_text(g, "1", false);
            }
        }
    }

    /// `CreateGemSlot(index)` (:743-1078).
    #[allow(clippy::too_many_lines)]
    fn create_gem_slot(&self, g: &mut Gfx, index: usize) {
        let me = self.me.clone();
        let tab = move || me.upgrade().expect("skillsTab");
        let prev = (index > 1).then(|| self.gem_slots.borrow()[index - 2].clone());
        let t = tab.clone();
        let delete = ButtonControl::new(None, rect(0.0, 0.0, 20.0, 20.0), "x".to_string(), move |g| t().delete_gem(g, index));
        match &prev {
            None => {
                let a: ControlRef = self.anchor_gem_slots.clone();
                delete
                    .base
                    .set_anchor("TOPLEFT", Some(Rc::downgrade(&a)), "TOPLEFT", Some((0.0, 0.0)), false);
            }
            Some(p) => {
                let a: ControlRef = p.delete.clone();
                delete
                    .base
                    .set_anchor("TOPLEFT", Some(Rc::downgrade(&a)), "BOTTOMLEFT", Some((0.0, 0.0)), false);
                let p2 = p.clone();
                delete.base.y.set_fn(move |g| {
                    if p2.enable_global1.is_shown(g) || p2.enable_global2.is_shown(g) {
                        24.0
                    } else {
                        2.0
                    }
                });
            }
        }
        let t = tab.clone();
        delete.base.shown.set_fn(move |_| {
            let s = t();
            s.display_index().is_some_and(|d| {
                s.read(|e| {
                    let x = &e.socket_groups()[d - 1];
                    index <= x.gem_list.len() + 1 && x.source.is_none()
                })
            })
        });
        let in_list = {
            let t = tab.clone();
            move |_: &Gfx| {
                let s = t();
                s.display_index()
                    .is_some_and(|d| s.read(|e| index <= e.socket_groups()[d - 1].gem_list.len()))
            }
        };
        delete.base.enabled.set_fn(in_list.clone());
        delete.tip.tooltip_text.set(Some("Remove this gem.".to_string()));
        self.host.add(&format!("gemSlot{index}Delete"), delete.clone());
        let dr: ControlRef = delete.clone();

        let slot_cell: Rc<RefCell<Option<Rc<GemSlot>>>> = Rc::new(RefCell::new(None));
        // :786-837
        let t = tab.clone();
        let sc = slot_cell.clone();
        let change: GemChangeFunc = Rc::new(move |g: &mut Gfx, gem_id: Option<String>, add_undo, focus_lost, buf_matches| {
            let s = t();
            let Some(group) = s.display_index() else { return };
            let slot = sc.borrow().clone().expect("slot");
            let exists = s.read(|e| e.socket_groups()[group - 1].gem_list.len()) >= index;
            if !exists {
                if gem_id.is_none() {
                    return;
                }
                s.edit(|e| e.add_empty_gem(group));
                let q = s.read(|e| e.skills_tab().default_gem_quality);
                slot.level.set_text(g, "1", false);
                slot.quality.set_text(g, &tostring_num(q), false);
                slot.enabled.state.set(true);
                slot.enable_global1.state.set(true);
                slot.enable_global2.state.set(true);
                slot.count.set_text(g, "1", false);
            } else if focus_lost && !buf_matches {
                s.delete_gem(g, index);
                return;
            } else if gem_id == s.read(|e| e.gem_id_at(group, index)) {
                if add_undo {
                    s.add_undo_state(false);
                }
                if buf_matches {
                    s.build().build_flag.set(true);
                }
                return;
            }
            let (level, count) = s.edit(|e| e.set_gem_id(group, index, gem_id.as_deref()));
            slot.level.set_text(g, &tostring_num(level), false);
            slot.count.set_text(g, &tostring_num(count), false);
            if add_undo {
                s.add_undo_state(false);
            }
            if buf_matches {
                s.build().build_flag.set(true);
            }
        });
        let name_spec = GemSelectControl::new(
            g,
            anchor("LEFT", &dr, "RIGHT"),
            rect(2.0, 0.0, 300.0, 20.0),
            self.me.clone(),
            index,
            change,
            true,
            false,
        );
        let glabel: ControlRef = self.group_label.clone();
        let nr: ControlRef = name_spec.clone();
        add_to_tab_group(&nr, &glabel);
        self.host.add(&format!("gemSlot{index}Name"), name_spec.clone());

        let num_edit = |g: &Gfx, a: &ControlRef, x: f64, f: Rc<dyn Fn(&mut Gfx, Option<f64>)>| {
            EditControl::new(
                g,
                anchor("LEFT", a, "RIGHT"),
                rect(x, 0.0, 60.0, 20.0),
                EditArgs {
                    filter: Some("%D".into()),
                    limit: Some(2),
                    change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _| f(g, buf.parse::<f64>().ok()))),
                    ..Default::default()
                },
            )
        };
        // :842-856
        let (t, sc) = (tab.clone(), slot_cell.clone());
        let level = num_edit(
            g,
            &nr,
            2.0,
            Rc::new(move |g, v| {
                let s = t();
                let group = s.display_index().expect("displayGroup");
                let slot = sc.borrow().clone().expect("slot");
                s.ensure_gem(g, &slot, group, index, false, true, true);
                s.edit(|e| e.set_gem_level(group, index, v));
                s.add_undo_state(false);
                s.build().build_flag.set(true);
            }),
        );
        let lr: ControlRef = level.clone();
        add_to_tab_group(&lr, &glabel);
        level.base.enabled.set_fn(in_list.clone());
        self.host.add(&format!("gemSlot{index}Level"), level.clone());
        // :864-878
        let (t, sc) = (tab.clone(), slot_cell.clone());
        let quality = num_edit(
            g,
            &lr,
            2.0,
            Rc::new(move |g, v| {
                let s = t();
                let group = s.display_index().expect("displayGroup");
                let slot = sc.borrow().clone().expect("slot");
                s.ensure_gem(g, &slot, group, index, true, false, true);
                s.edit(|e| e.set_gem_quality(group, index, v));
                s.add_undo_state(false);
                s.build().build_flag.set(true);
            }),
        );
        let t = tab.clone();
        *quality.border_func.borrow_mut() = Some(Rc::new(move || {
            let s = t();
            let Some(d) = s.display_index() else { return (0.5, 0.5, 0.5) };
            s.read(|e| {
                let gem = e.socket_groups()[d - 1].gem_list.get(index - 1);
                match gem.filter(|x| x.matches_socket == Some(true)).and_then(|x| x.color.as_deref()) {
                    Some(c) => hex_to_rgb(&c.replace('^', "0")).map_or((0.5, 0.5, 0.5), |v| (v[0], v[1], v[2])),
                    None => (0.5, 0.5, 0.5),
                }
            })
        }));
        let t = tab.clone();
        quality
            .tip
            .set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| t().quality_tooltip(tooltip, g, index));
        let qr: ControlRef = quality.clone();
        add_to_tab_group(&qr, &glabel);
        quality.base.enabled.set_fn(in_list.clone());
        self.host.add(&format!("gemSlot{index}Quality"), quality.clone());
        // :955-974
        let (t, sc) = (tab.clone(), slot_cell.clone());
        let enabled = CheckBoxControl::new(
            g,
            anchor("LEFT", &qr, "RIGHT"),
            [18.0, 0.0, 20.0],
            None,
            Some(Rc::new(move |g: &mut Gfx, state: bool| {
                let s = t();
                let group = s.display_index().expect("displayGroup");
                let slot = sc.borrow().clone().expect("slot");
                s.ensure_gem(g, &slot, group, index, true, true, true);
                if s.edit(|e| e.set_gem_enabled(group, index, state)) {
                    slot.enable_global1.state.set(true);
                    slot.enable_global2.state.set(true);
                }
                s.add_undo_state(false);
                s.build().build_flag.set(true);
            })),
            None,
            false,
        );
        let t = tab.clone();
        enabled
            .tip
            .set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, _| t().enabled_tooltip(tooltip, g, index));
        enabled.base.enabled.set_fn(in_list.clone());
        self.host.add(&format!("gemSlot{index}Enable"), enabled.clone());
        let er: ControlRef = enabled.clone();
        // :994-1032
        let (t, sc) = (tab.clone(), slot_cell.clone());
        let count = num_edit(
            g,
            &er,
            18.0,
            Rc::new(move |g, v| {
                let s = t();
                let group = s.display_index().expect("displayGroup");
                let slot = sc.borrow().clone().expect("slot");
                s.ensure_gem(g, &slot, group, index, true, true, false);
                let shown = s.edit(|e| e.set_gem_count(group, index, v));
                *slot.count.buf.borrow_mut() = tostring_num(shown);
                s.add_undo_state(false);
                s.build().build_flag.set(true);
            }),
        );
        let t = tab.clone();
        count.base.shown.set_fn(move |_| {
            let s = t();
            let Some(d) = s.display_index() else { return false };
            s.read(|e| {
                let Some(gem) = e.socket_groups()[d - 1].gem_list.get(index - 1) else {
                    return false;
                };
                let skills = pob_data::skills::skills();
                let list: Vec<&pob_data::skills::GrantedEffect> = match gem.gem_data.as_deref().and_then(gem_of) {
                    Some(gd) => std::iter::once(gd.granted_effect_id.as_str())
                        .chain(gd.secondary_granted_effect_id.as_deref())
                        .filter_map(|i| skills.get(i))
                        .collect(),
                    None => gem.granted_effect.as_deref().and_then(|i| skills.get(i)).into_iter().collect(),
                };
                list.iter().enumerate().any(|(k, ge)| {
                    let global = if k == 0 { gem.enable_global1 } else { gem.enable_global2 };
                    !ge.support && (!ge.has_global_effect.get() || global == Some(true))
                })
            })
        });
        let t = tab.clone();
        count.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| {
            let s = t();
            let Some(d) = s.display_index() else { return };
            let rev = s.read(Engine::output_revision);
            if tooltip.check_for_update((rev, d)) {
                tooltip.add_line(
                    g,
                    16.0,
                    Some("^8Note: `count` integer value scales the DPS of associated skill by a scalar."),
                );
                tooltip.add_line(
                    g,
                    16.0,
                    Some("^8To be used with totems, minions, shot-gunning of projectiles (e.g., VD, magma-orbs),"),
                );
                tooltip.add_line(g, 16.0, Some("^8multi-hit projectiles (e.g. ball-lightning), traps, mines."));
            }
        });
        count.base.enabled.set_fn(in_list.clone());
        self.host.add(&format!("gemSlot{index}Count"), count.clone());
        let cr: ControlRef = count.clone();
        // :1035-1039
        let t = tab.clone();
        let err_msg = LabelControl::new(
            anchor("LEFT", &cr, "RIGHT"),
            rect(2.0, 2.0, 0.0, 16.0),
            Prop::func(move |_| {
                let s = t();
                let msg = s
                    .display_index()
                    .and_then(|d| s.read(|e| e.socket_groups()[d - 1].gem_list.get(index - 1).and_then(|x| x.err_msg.clone())));
                format!("^1{}", msg.unwrap_or_default())
            }),
        );
        self.host.add(&format!("gemSlot{index}ErrMsg"), err_msg.clone());
        // :1042-1077
        let global = |which: u8, a: Option<crate::control::Anchor>, g: &Gfx, t: Rc<dyn Fn() -> Rc<SkillsTab>>| {
            let t2 = t.clone();
            let c = CheckBoxControl::new(
                g,
                a,
                [0.0, if which == 1 { 2.0 } else { 0.0 }, 20.0],
                Some(""),
                Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                    let s = t2();
                    let group = s.display_index().expect("displayGroup");
                    s.edit(|e| e.set_gem_enable_global(group, index, which, state));
                    s.add_undo_state(false);
                    s.build().build_flag.set(true);
                })),
                None,
                false,
            );
            let t2 = t.clone();
            let effect = move |s: &SkillsTab| -> Option<String> {
                let d = s.display_index()?;
                s.read(|e| {
                    let gem = e.socket_groups()[d - 1].gem_list.get(index - 1)?;
                    let gd = gem.gem_data.as_deref().and_then(gem_of).filter(|x| x.vaal_gem == Some(true))?;
                    let skills = pob_data::skills::skills();
                    let id = if which == 1 {
                        Some(gd.granted_effect_id.as_str())
                    } else {
                        gd.secondary_granted_effect_id.as_deref()
                    }?;
                    skills.get(id).filter(|x| !x.support).map(|x| x.name.clone())
                })
            };
            let e2 = effect;
            c.base.shown.set_fn(move |_| e2(&t2()).is_some());
            let t2 = t.clone();
            let e2 = effect;
            c.label.set_fn(move |_| Some(format!("Enable {}:", e2(&t2()).unwrap_or_default())));
            let w = Rc::downgrade(&c);
            let extra = if which == 1 { 5.0 } else { 12.0 };
            c.base.x.set_fn(move |g| {
                let l = w.upgrade().expect("checkbox").label.get(g).unwrap_or_default();
                sg::draw_string_width(g, 16.0, "VAR", &l) + extra
            });
            c
        };
        let tf: Rc<dyn Fn() -> Rc<SkillsTab>> = Rc::new(tab);
        let enable_global1 = global(1, anchor("TOPLEFT", &dr, "BOTTOMLEFT"), g, tf.clone());
        self.host.add(&format!("gemSlot{index}EnableGlobal1"), enable_global1.clone());
        let g1: ControlRef = enable_global1.clone();
        let mut a2 = anchor("LEFT", &g1, "RIGHT");
        if let Some(a) = a2.as_mut() {
            a.collapse = true;
        }
        let enable_global2 = global(2, a2, g, tf);
        self.host.add(&format!("gemSlot{index}EnableGlobal2"), enable_global2.clone());
        let slot = Rc::new(GemSlot {
            delete,
            name_spec,
            level,
            quality,
            enabled,
            count,
            err_msg,
            enable_global1,
            enable_global2,
        });
        *slot_cell.borrow_mut() = Some(slot.clone());
        self.gem_slots.borrow_mut().push(slot);
    }

    /// `slot.quality.tooltipFunc` (:888-947).
    fn quality_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, index: usize) {
        let Some(d) = self.display_index() else { return };
        let token = self.display_token();
        self.read(|e| {
            if !tooltip.check_for_update((e.output_revision(), token.clone())) {
                return;
            }
            let Some(gi) = e.socket_groups()[d - 1].gem_list.get(index - 1) else {
                return;
            };
            let skills = pob_data::skills::skills();
            let gd = gi.gem_data.as_deref().and_then(gem_of);
            let mut add = |ge: &pob_data::skills::GrantedEffect| {
                let Some(q) = ge.quality_stats.as_ref().filter(|q| !q.is_empty()) else {
                    return;
                };
                tooltip.add_line(g, 18.0, Some(&format!("{}{}", cc("GEM"), ge.name)));
                tooltip.add_line(g, 16.0, Some(&format!("{}At +20% Quality:", cc("NORMAL"))));
                let scope = ge.stat_description_scope.as_deref().unwrap_or("skill_stat_descriptions");
                for (stat, v) in q {
                    let (lines, _) = crate::stat_describer::describe_stats(&[(stat.clone(), v * 20.0)], scope, false);
                    for line in lines {
                        if skills.stat_map(ge, stat).unwrap_or_else(|er| panic!("{er}")).is_some() {
                            tooltip.add_line(g, 16.0, Some(&format!("{}{line}", cc("MAGIC"))));
                        } else {
                            let mut l = format!("{}{line}", cc("UNSUPPORTED"));
                            if crate::common::not_supported_mod_tooltips() {
                                l.push_str(" ^8(Not supported in PoB yet)");
                            }
                            tooltip.add_line(g, 16.0, Some(&l));
                        }
                    }
                }
                tooltip.add_separator(Some(10.0));
            };
            if let Some(gd) = gd {
                if let Some(ge) = skills.get(&gd.granted_effect_id) {
                    add(ge);
                }
                if let Some(ge) = gd.secondary_granted_effect_id.as_deref().and_then(|i| skills.get(i)) {
                    add(ge);
                }
            }
            let lines = e
                .gem_stat_compare(d, index, &GemEdit::Quality20, "^7Setting to 20 quality will give you:", None)
                .unwrap_or_else(|er| panic!("{er}"));
            for l in lines {
                tooltip.add_line(g, 14.0, Some(&l));
            }
        });
    }

    /// `slot.enabled.tooltipFunc` (:975-987).
    fn enabled_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, index: usize) {
        let Some(d) = self.display_index() else { return };
        let token = self.display_token();
        self.read(|e| {
            if !tooltip.check_for_update((e.output_revision(), token.clone())) {
                return;
            }
            let Some(gi) = e.socket_groups()[d - 1].gem_list.get(index - 1) else {
                return;
            };
            let header = if gi.enabled {
                "^7Disabling this gem will give you:"
            } else {
                "^7Enabling this gem will give you:"
            };
            let lines = e
                .gem_stat_compare(d, index, &GemEdit::ToggleEnabled, header, None)
                .unwrap_or_else(|er| panic!("{er}"));
            for l in lines {
                tooltip.add_line(g, 14.0, Some(&l));
            }
        });
    }

    /// `getSelectedItem` (:218-231): `(item, slotName)` of `groupSlot`'s selection.
    fn selected_item(&self) -> Option<(Rc<pob_items::Item>, String)> {
        let v = self.group_slot.get_sel_value()?;
        let slot: Option<String> = data_of(&v);
        let slot = slot?;
        self.read(|e| e.slot_item(&slot)).map(|i| (i, slot))
    }

    /// `controls.sourceNote.label` (:387-417).
    fn source_note_text(&self) -> String {
        let Some(d) = self.display_index() else { return String::new() };
        self.read(|e| e.source_note(d)).unwrap_or_else(|er| panic!("{er}"))
    }

    /// `:CopySocketGroup` is in the list's hooks; `:PasteSocketGroup` (:708-740).
    fn paste_socket_group(&self, g: &mut Gfx) {
        let Some(text) = g.paste() else { return };
        if let Some(i) = self.edit(|e| e.paste_socket_group(&text)) {
            self.sync_list();
            self.group_list.sel_index.set(Some(i));
            *self.group_list.sel_value.borrow_mut() = self.group_list.list.borrow().get(i - 1).cloned();
            self.set_display_group(g, Some(i));
            self.add_undo_state(false);
            self.build().build_flag.set(true);
        }
    }

    /// `RestoreUndoState` (:1421-1439).
    fn restore(&self, g: &mut Gfx, state: &SkillsUndoState) {
        let display_id = self.display_index();
        self.edit(|e| e.restore_skills_undo_state(state));
        *self.group_list.list.borrow_mut() = Vec::new();
        self.sync_list();
        self.build().sync_loadouts_pub(g);
        // `SetActiveSkillSet`: `SetDisplayGroup(socketGroupList[1])`, then `SetDisplayGroup(displayId and socketGroupList[displayId])`
        let first = (!self.group_list.list.borrow().is_empty()).then_some(1);
        self.set_display_group(g, first);
        self.set_display_group(g, display_id.filter(|i| *i <= self.group_list.list.borrow().len()));
        if self.group_list.sel_value.borrow().is_some() {
            let v = self
                .group_list
                .sel_index
                .get()
                .and_then(|i| self.group_list.list.borrow().get(i - 1).cloned());
            *self.group_list.sel_value.borrow_mut() = v;
        }
    }

    /// UndoHandler.lua:41-47
    fn undo_g(&self, g: &mut Gfx) {
        if self.undo.undo.borrow().len() >= 2 {
            let top = self.undo.undo.borrow_mut().remove(0);
            self.undo.redo.borrow_mut().insert(0, top);
            let s = self.undo.undo.borrow_mut().remove(0);
            self.restore(g, &s);
            self.add_undo_state(true);
        }
    }

    /// UndoHandler.lua:50-55
    fn redo_g(&self, g: &mut Gfx) {
        if !self.undo.redo.borrow().is_empty() {
            let s = self.undo.redo.borrow_mut().remove(0);
            self.restore(g, &s);
            self.add_undo_state(true);
        }
    }

    /// SkillsTab.lua:256-289
    fn optimise_sockets(&self, g: &mut Gfx) {
        let Some((item, slot)) = self.selected_item() else { return };
        let b = self.build();
        let tab = b.items_tab.borrow().clone().expect("itemsTab");
        tab.add_undo_state(false);
        let Some(base) = item.base else { return };
        // `getSocketCounts` (:246-255)
        let mut it = (*item).clone();
        let abyssal = it.sockets.iter().filter(|s| s.color == "A").count() as f64;
        let mut max_sockets = base.socket_limit.unwrap_or(0.0) - abyssal;
        let mut group_count = 0.0;
        it.sockets.clear();
        let skills = pob_data::skills::skills();
        self.read(|e| {
            for g2 in e.socket_groups() {
                if g2.slot.as_deref() == Some(slot.as_str()) {
                    for gem in &g2.gem_list {
                        let ge = gem
                            .granted_effect
                            .as_ref()
                            .and_then(|i| skills.get(i))
                            .or_else(|| gem.gem_data.as_deref().and_then(gem_of).and_then(|d| skills.get(&d.granted_effect_id)));
                        if let Some(ge) = ge
                            && max_sockets > 0.0
                        {
                            let colour = match ge.color {
                                Some(1.0) => "R",
                                Some(2.0) => "G",
                                Some(3.0) => "B",
                                _ => "W",
                            };
                            it.sockets.push(pob_items::Socket {
                                color: colour.to_string(),
                                group: group_count,
                            });
                            max_sockets -= 1.0;
                        }
                    }
                    group_count += 1.0;
                }
            }
        });
        for _ in 0..abyssal as usize {
            group_count += 1.0;
            it.sockets.push(pob_items::Socket {
                color: "A".to_string(),
                group: group_count,
            });
        }
        it.build_and_parse_raw();
        self.edit(|e| e.add_item(&it, None));
        tab.populate_slots(g);
        b.build_flag.set(true);
    }

    /// `SkillSetListControl:OnSelDelete`'s confirm (SkillSetListControl.lua:106-121).
    fn confirm_delete_set(&self, g: &mut Gfx, index: usize, id: f64) {
        let title = self
            .read(Engine::skill_sets)
            .into_iter()
            .find(|s| s.0 == id)
            .and_then(|s| s.1)
            .unwrap_or_else(|| "Default".to_string());
        let w = self.me.clone();
        main().open_confirm_popup(
            g,
            "Delete Item Set",
            &format!("Are you sure you want to delete '{title}'?"),
            "Delete",
            Rc::new(move |g: &mut Gfx| {
                let s = w.upgrade().expect("skillsTab");
                let l = s.set_list.borrow().upgrade().expect("set list");
                l.list.borrow_mut().remove(index - 1);
                let active = s.read(|e| e.skills_tab().active_skill_set_id);
                s.edit(|e| e.delete_skill_set(id));
                l.sel_index.set(None);
                *l.sel_value.borrow_mut() = None;
                if id == active {
                    let next = l.list.borrow()[index.saturating_sub(1).max(1) - 1];
                    s.set_active_skill_set(g, next);
                }
                s.add_undo_state(false);
                s.build().sync_loadouts_pub(g);
            }),
            None,
        );
    }

    /// `RenameSet(skillSet, addOnName)` (SkillSetListControl.lua:60-86).
    fn rename_set_popup(&self, g: &mut Gfx, what: RenameSet) {
        let title = match &what {
            RenameSet::Existing(id) | RenameSet::Copy(id) => {
                self.read(Engine::skill_sets).into_iter().find(|s| s.0 == *id).and_then(|s| s.1)
            }
            RenameSet::New => None,
        };
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "label".into(),
            LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter name for this skill set:".to_string()),
        );
        let save_slot: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let sl = save_slot.clone();
        let edit = EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 350.0, 20.0),
            EditArgs {
                init: title.clone(),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    if let Some(s) = &*sl.borrow() {
                        s.base.enabled.set(buf.bytes().any(|c| !c.is_ascii_whitespace()));
                    }
                })),
                ..Default::default()
            },
        );
        let (w, e2) = (self.me.clone(), edit.clone());
        let save = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
            let s = w.upgrade().expect("skillsTab");
            let text = e2.buf.borrow().clone();
            let new_id = match what {
                RenameSet::Existing(id) => {
                    s.edit(|e| e.rename_skill_set(id, &text));
                    None
                }
                RenameSet::New => Some(s.edit(|e| e.new_skill_set(&text))),
                RenameSet::Copy(id) => Some(s.edit(|e| e.copy_skill_set(id, &text))),
            };
            s.undo.mod_flag.set(true);
            if let Some(id) = new_id {
                let l = s.set_list.borrow().upgrade().expect("set list");
                l.list.borrow_mut().push(id);
                let n = l.list.borrow().len();
                l.sel_index.set(Some(n));
                *l.sel_value.borrow_mut() = Some(id);
            }
            s.add_undo_state(false);
            s.build().sync_loadouts_pub(g);
            main().close_popup();
        });
        save.base.enabled.set(false);
        *save_slot.borrow_mut() = Some(save.clone());
        controls.insert("edit".into(), edit);
        controls.insert("save".into(), save);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_| main().close_popup()),
        );
        let t = if title.is_some() { "Rename" } else { "Set Name" };
        main().open_popup(g, 370.0, 100.0, t, controls, Some("save"), Some("edit"), Some("cancel"), None);
    }

    /// `OpenSkillSetManagePopup` (:1442-1449) + `SkillSetListControl:SkillSetListControl` (SkillSetListControl.lua:17-58).
    fn open_manage_popup(&self, g: &mut Gfx) {
        let ids: Vec<f64> = self.read(Engine::skill_sets).into_iter().map(|s| s.0).collect();
        let list = ListControl::new(
            None,
            rect(0.0, 50.0, 350.0, 200.0),
            16.0,
            Some("VERTICAL"),
            true,
            ids,
            false,
            Box::new(SetListHooks(self.me.clone())),
        );
        *self.set_list.borrow_mut() = Rc::downgrade(&list);
        let lr: ControlRef = list.clone();
        let (me, wl) = (self.me.clone(), Rc::downgrade(&list));
        let tab = move || me.upgrade().expect("skillsTab");
        let l2 = wl.clone();
        let t = tab.clone();
        let copy = ButtonControl::new(
            anchor("BOTTOMLEFT", &lr, "TOP"),
            rect(2.0, -4.0, 60.0, 18.0),
            "Copy".to_string(),
            move |g| {
                let id = l2.upgrade().expect("list").sel_value.borrow().expect("selValue");
                t().rename_set_popup(g, RenameSet::Copy(id));
            },
        );
        let l2 = wl.clone();
        copy.base
            .enabled
            .set_fn(move |_| l2.upgrade().expect("list").sel_value.borrow().is_some());
        let copy_r: ControlRef = copy.clone();
        let (l2, t) = (wl.clone(), tab.clone());
        let delete = ButtonControl::new(
            anchor("LEFT", &copy_r, "RIGHT"),
            rect(4.0, 0.0, 60.0, 18.0),
            "Delete".to_string(),
            move |g| {
                let l = l2.upgrade().expect("list");
                let (i, id) = (l.sel_index.get().expect("selIndex"), l.sel_value.borrow().expect("selValue"));
                if l.list.borrow().len() > 1 {
                    t().confirm_delete_set(g, i, id);
                }
            },
        );
        let l2 = wl.clone();
        delete.base.enabled.set_fn(move |_| {
            let l = l2.upgrade().expect("list");
            let n = l.list.borrow().len();
            l.sel_value.borrow().is_some() && n > 1
        });
        let (l2, t) = (wl.clone(), tab.clone());
        let rename = ButtonControl::new(
            anchor("BOTTOMRIGHT", &lr, "TOP"),
            rect(-2.0, -4.0, 60.0, 18.0),
            "Rename".to_string(),
            move |g| {
                let id = l2.upgrade().expect("list").sel_value.borrow().expect("selValue");
                t().rename_set_popup(g, RenameSet::Existing(id));
            },
        );
        let l2 = wl.clone();
        rename
            .base
            .enabled
            .set_fn(move |_| l2.upgrade().expect("list").sel_value.borrow().is_some());
        let rn: ControlRef = rename.clone();
        let t = tab.clone();
        let new_btn = ButtonControl::new(
            anchor("RIGHT", &rn, "LEFT"),
            rect(-4.0, 0.0, 60.0, 18.0),
            "New".to_string(),
            move |g| {
                t().rename_set_popup(g, RenameSet::New);
            },
        );
        list.host.add("copy", copy);
        list.host.add("delete", delete);
        list.host.add("rename", rename);
        list.host.add("new", new_btn);
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert("1".into(), list);
        controls.insert(
            "2".into(),
            ButtonControl::new(None, rect(0.0, 260.0, 90.0, 20.0), "Done".to_string(), |_| main().close_popup()),
        );
        main().open_popup(g, 370.0, 290.0, "Manage Skill Sets", controls, None, None, None, None);
    }

    /// `SkillsTabClass:Draw` (:631-692).
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        self.sync_list();
        let a = &self.anchor.base;
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        let sb = &self.scroll_bar_h;
        sb.base.width.set(view_port.width);
        sb.base.x.set(view_port.x);
        sb.base.y.set(view_port.y + view_port.height - 18.0);
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        if let Some(h) = self.host.get("gemCountHeader") {
            let max_x = h.get_pos(f).0 + h.get_size(f).0 + 350.0;
            sb.set_content_dimension(f, max_x - view_port.x, view_port.width);
        }
        a.x.set(view_port.x - sb.offset.get());
        let keys: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in keys {
            if sg::is_key_down(f, "CTRL") {
                match key.as_str() {
                    "z" => {
                        self.undo_g(f);
                        self.build().build_flag.set(true);
                    }
                    "y" => {
                        self.redo_g(f);
                        self.build().build_flag.set(true);
                    }
                    "v" => self.paste_socket_group(f),
                    _ => {}
                }
            }
        }
        self.host.process_controls_input(f, input_events, view_port);
        let pending = self.pending_focus.borrow_mut().take();
        if let Some(c) = pending {
            self.host.select_control(f, Some(c));
        }
        let ups: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyUp { key } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in ups {
            if ScrollBarControl::is_scroll_down_key(&key) {
                sb.scroll(1.0);
            } else if ScrollBarControl::is_scroll_up_key(&key) {
                sb.scroll(-1.0);
            }
        }
        main().draw_background(f, view_port);
        let (sets, active) = self.read(|e| (e.skill_sets(), e.skills_tab().active_skill_set_id));
        let list: Vec<DropItem> = sets.iter().map(|(_, t)| DropItem::str(t.as_deref().unwrap_or("Default"))).collect();
        if let Some(i) = sets.iter().position(|s| s.0 == active) {
            self.set_select.sel_index.set(i + 1);
        }
        self.set_select.set_list(f, Some(list));
        let gl: ControlRef = self.group_list.clone();
        let adc = &self.anchor_group_detail.base;
        if main().portrait_mode.get() {
            let os = self.host.get("optionSection").expect("optionSection");
            adc.set_anchor("TOPLEFT", Some(Rc::downgrade(&os)), "BOTTOMLEFT", Some((0.0, 20.0)), false);
        } else {
            adc.set_anchor("TOPLEFT", Some(Rc::downgrade(&gl)), "TOPRIGHT", Some((20.0, 0.0)), false);
        }
        self.sync_list();
        self.update_gem_slots(f);
        let sel = self.host.sel();
        self.host.draw_controls(f, view_port, sel.as_ref());
    }

    /// The state the gate compares (the same fields as `skills-runner.lua`).
    #[allow(clippy::too_many_lines)]
    pub fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        use serde_json::json;
        let drop = |c: &DropDownControl| {
            let list: Vec<String> = c.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect();
            json!({ "shown": c.is_shown(g), "enabled": c.is_shown(g) && c.is_enabled(g), "sel": c.sel_index.get(), "list": list })
        };
        let n = self.group_list.list.borrow().len();
        let rows: Vec<String> = {
            let l = self.group_list.list.borrow().clone();
            l.iter()
                .enumerate()
                .map(|(i, v)| self.group_list.hooks.get_row_value(&self.group_list, g, 1, i + 1, v))
                .collect()
        };
        let d = self.display_index();
        let num = |o: Option<f64>| o.map_or(serde_json::Value::Null, |v| json!(v));
        let groups = self.read(|e| {
            let gems = |x: &pob_calc::skills_tab::SocketGroup| -> Vec<serde_json::Value> {
                x.gem_list
                    .iter()
                    .map(|i| {
                        json!({ "name": i.name_spec, "gem": i.gem_data, "skill": i.skill_id, "level": num(i.level), "quality": num(i.quality), "enabled": i.enabled,
                            "g1": i.enable_global1.unwrap_or(false), "g2": i.enable_global2.unwrap_or(false), "count": num(i.count), "color": i.color, "err": i.err_msg, "match": i.matches_socket == Some(true) })
                    })
                    .collect()
            };
            e.socket_groups()
                .iter()
                .map(|x| {
                    json!({ "label": x.label, "display": x.display_label, "slot": x.slot, "source": x.source, "enabled": x.enabled, "fullDPS": x.include_in_full_dps == Some(true),
                        "count": num(x.group_count), "imbued": x.imbued_support, "main": num(x.main_active_skill), "gems": gems(x) })
                })
                .collect::<Vec<_>>()
        });
        let slots: Vec<serde_json::Value> = {
            let gem_n = d.map_or(0, |i| self.read(|e| e.socket_groups()[i - 1].gem_list.len()));
            self.gem_slots
                .borrow()
                .iter()
                .take(gem_n + 1)
                .map(|s| {
                    let (x, y) = s.name_spec.get_pos(g);
                    json!({
                        "name": s.name_spec.edit.buf.borrow().clone(), "col": s.name_spec.edit.inactive_col.borrow().clone(),
                        "level": s.level.buf.borrow().clone(), "quality": s.quality.buf.borrow().clone(), "enabled": s.enabled.state.get(),
                        "countShown": s.count.is_shown(g), "count": s.count.buf.borrow().clone(), "err": s.err_msg.label.get(g),
                        "g1": s.enable_global1.is_shown(g), "g2": s.enable_global2.is_shown(g), "g1s": s.enable_global1.state.get(), "g2s": s.enable_global2.state.get(),
                        "g1l": if s.enable_global1.is_shown(g) { s.enable_global1.label.get(g) } else { None },
                        "g2l": if s.enable_global2.is_shown(g) { s.enable_global2.label.get(g) } else { None },
                        "delShown": s.delete.is_shown(g), "delEnabled": s.delete.is_shown(g) && s.delete.is_enabled(g), "levelEnabled": s.level.is_shown(g) && s.level.is_enabled(g),
                        "pos": format!("{x},{y}"),
                    })
                })
                .collect()
        };
        let select = self.host.sel().and_then(|s| self.gem_select_of(&s)).map(|s| {
            let cache = s.sort_cache.borrow();
            let list = s.list.borrow().clone();
            let names: Vec<String> = list
                .iter()
                .map(|id| {
                    let name = gem_of(id).map_or("<No matches>", |x| x.name.as_str());
                    let col = cache.as_ref().and_then(|c| c.dps_color.get(id)).copied().unwrap_or("");
                    let sup = cache.as_ref().is_some_and(|c| c.can_support.contains(id));
                    format!("{name}|{col}|{}", if sup { "1" } else { "0" })
                })
                .collect();
            json!({ "dropped": s.dropped.get(), "sel": s.sel_index.get(), "buf": s.edit.buf.borrow().clone(), "noMatches": s.no_matches.get(), "hover": s.hover_sel.get(),
                "search": s.search_str.borrow().clone(), "list": names, "scroll": s.scroll_bar.offset.get(), "tooltip": s.debug_tooltip() })
        });
        let imbued = self.imbued();
        let list_tip: Vec<String> = self
            .group_list
            .tooltip
            .borrow()
            .lines
            .iter()
            .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
            .collect();
        let tip_of = |t: &crate::tooltip::Tooltip| -> Vec<String> {
            t.lines
                .iter()
                .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
                .collect()
        };
        let hover_tips: Vec<serde_json::Value> = self
            .gem_slots
            .borrow()
            .iter()
            .map(|s| json!({ "q": tip_of(&s.quality.tip.tooltip.borrow()), "e": tip_of(&s.enabled.tip.tooltip.borrow()), "n": s.name_spec.debug_tooltip() }))
            .collect();
        let (sets, active, opts) = self.read(|e| {
            let m = &e.model().skills;
            (
                e.skill_sets()
                    .into_iter()
                    .map(|s| json!({ "id": s.0, "title": s.1 }))
                    .collect::<Vec<_>>(),
                e.skills_tab().active_skill_set_id,
                json!({ "level": e.skills_tab().default_gem_level, "quality": e.skills_tab().default_gem_quality, "sort": m.sort_gems_by_dps.unwrap_or(true),
                    "field": m.sort_gems_by_dps_field, "support": m.show_support_gem_types, "legacy": m.show_legacy_gems.unwrap_or(false), "spectres": e.model().spectres }),
            )
        });
        let sel_value = self
            .group_list
            .sel_value
            .borrow()
            .as_ref()
            .and_then(|v| self.group_list.list.borrow().iter().position(|x| x == v).map(|i| i + 1));
        let sel_name = self.host.sel().map_or("nil".to_string(), |c| {
            let names = self.host.controls.borrow();
            names
                .iter()
                .find(|(_, v)| same(&***v, &*c))
                .map_or("other".to_string(), |(k, _)| k.clone())
        });
        json!({
            "n": n, "rows": rows, "groups": groups, "display": d, "selIndex": self.group_list.sel_index.get(), "selValue": sel_value, "sets": sets, "active": active, "opts": opts,
            "setSelect": drop(&self.set_select), "label": self.group_label.buf.borrow().clone(), "slot": drop(&self.group_slot),
            "groupEnabled": self.group_enabled.state.get(), "fullDPS": self.include_in_full_dps.state.get(),
            "countShown": d.is_some() && self.group_count.is_shown(g), "groupCount": self.group_count.buf.borrow().clone(),
            "socketsShown": d.is_some() && self.sockets_label.is_shown(g), "sockets": if d.is_some() && self.sockets_label.is_shown(g) { Some(self.sockets_label.label.get(g)) } else { None },
            "optimiseShown": d.is_some() && self.optimise_sockets.is_shown(g), "optimiseEnabled": d.is_some() && self.optimise_sockets.is_shown(g) && self.optimise_sockets.is_enabled(g),
            "noteShown": d.is_some() && self.source_note.is_shown(g), "note": if d.is_some() && self.source_note.is_shown(g) { Some(self.source_note.label.get(g)) } else { None },
            "imbued": { "shown": d.is_some() && self.imbued_label.is_shown(g), "enabled": d.is_some() && imbued.is_enabled(g), "buf": imbued.edit.buf.borrow().clone(), "col": imbued.edit.inactive_col.borrow().clone() },
            "slots": slots, "select": select, "listTip": list_tip, "tips": hover_tips, "undo": self.undo.undo.borrow().len(), "redo": self.undo.redo.borrow().len(), "sel": sel_name,
            "scrollH": self.scroll_bar_h.offset.get(), "defaultLevel": self.default_level.sel_index.get(), "sortField": self.sort_field.sel_index.get(),
            "defaultQuality": self.default_quality.buf.borrow().clone(), "showSupport": self.show_support.sel_index.get(), "sortByDps": self.sort_gems_by_dps.state.get(), "showLegacy": self.show_legacy.state.get(),
        })
    }

    /// The centre of a named control (`cursorControl` of the gate); `gemSlot<i><Name>` included.
    pub fn debug_control_pos(&self, g: &Gfx, name: &str, dx: f64, dy: f64) -> (f64, f64) {
        let c: ControlRef = match name {
            "imbuedSupport" => self.imbued(),
            "groupList" => self.group_list.clone(),
            _ => self.host.get(name).unwrap_or_else(|| panic!("control {name}")),
        };
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        ((x + w / 2.0).floor() + dx, (y + h / 2.0).floor() + dy)
    }
}

/// `new("SkillsTab")` (:82-438) + the `Load` of the model's options.
#[allow(clippy::too_many_lines)]
pub fn new_skills_tab(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<SkillsTab> {
    let (opts, default_gem_level, default_quality) = {
        let b = build.upgrade().expect("buildMode");
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        (
            e.model().skills.clone(),
            e.model().skills.default_gem_level.clone(),
            e.model().skills.default_gem_quality,
        )
    };
    let tab = Rc::new_cyclic(|me: &Weak<SkillsTab>| {
        let w = me.clone();
        let t = move || w.upgrade().expect("skillsTab");
        let anchor_c = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
        let ar: ControlRef = anchor_c.clone();
        let host = ControlHost::new();
        let flag = {
            let t = t.clone();
            move || {
                let s = t();
                s.add_undo_state(false);
                s.build().build_flag.set(true);
            }
        };
        let t1 = t.clone();
        let set_select = DropDownControl::new(
            g,
            anchor("TOPLEFT", &ar, "TOPLEFT"),
            rect(76.0, 8.0, 210.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                let id = s.read(Engine::skill_sets)[index - 1].0;
                s.set_active_skill_set(g, id);
                s.add_undo_state(false);
            })),
            None,
            false,
        );
        set_select.enable_dropped_width.set(true);
        let t1 = t.clone();
        set_select.base.enabled.set_fn(move |_| t1().read(|e| e.skill_sets().len() > 1));
        let sr: ControlRef = set_select.clone();
        let set_label = LabelControl::new(anchor("RIGHT", &sr, "LEFT"), rect(-2.0, 0.0, 0.0, 16.0), "^7Skill set:".to_string());
        let t1 = t.clone();
        let set_manage = ButtonControl::new(
            anchor("LEFT", &sr, "RIGHT"),
            rect(4.0, 0.0, 90.0, 20.0),
            "Manage...".to_string(),
            move |g| {
                t1().open_manage_popup(g);
            },
        );
        host.add("setSelect", set_select.clone());
        host.add("setLabel", set_label);
        host.add("setManage", set_manage);

        // :113-122, SkillListControl.lua:35-77
        let group_list = ListControl::new(
            anchor("TOPLEFT", &ar, "TOPLEFT"),
            rect(20.0, 54.0, 360.0, 300.0),
            16.0,
            Some("VERTICAL"),
            true,
            Vec::new(),
            false,
            Box::new(SkillListHooks(me.clone())),
        );
        group_list.label.set(Some("^7Socket Groups:".to_string()));
        let glr: ControlRef = group_list.clone();
        let t1 = t.clone();
        let del = ButtonControl::new(
            anchor("BOTTOMRIGHT", &glr, "TOPRIGHT"),
            rect(0.0, -2.0, 60.0, 18.0),
            "Delete".to_string(),
            move |g| {
                let s = t1();
                let l = s.group_list.clone();
                let v = l.sel_value.borrow().clone();
                if let (Some(i), Some(v)) = (l.sel_index.get(), v) {
                    l.hooks.on_sel_delete(&l, g, i, &v);
                }
            },
        );
        let t1 = t.clone();
        del.base.enabled.set_fn(move |_| {
            let s = t1();
            let l = &s.group_list;
            let v = l.sel_value.borrow().clone();
            v.and_then(|v| l.list.borrow().iter().position(|x| *x == v))
                .is_some_and(|i| s.read(|e| e.socket_groups().get(i).is_some_and(|x| x.source.is_none())))
        });
        let dr: ControlRef = del.clone();
        let t1 = t.clone();
        let del_all = ButtonControl::new(
            anchor("RIGHT", &dr, "LEFT"),
            rect(-4.0, 0.0, 70.0, 18.0),
            "Delete All".to_string(),
            move |g| {
                let t2 = t1.clone();
                main().open_confirm_popup(
                    g,
                    "Delete All",
                    "Are you sure you want to delete all socket groups in this build?",
                    "Delete",
                    Rc::new(move |g: &mut Gfx| {
                        let s = t2();
                        s.group_list.list.borrow_mut().clear();
                        s.edit(Engine::delete_all_socket_groups);
                        s.set_display_group(g, None);
                        s.add_undo_state(false);
                        s.build().build_flag.set(true);
                        s.group_list.sel_index.set(None);
                        *s.group_list.sel_value.borrow_mut() = None;
                    }),
                    None,
                );
            },
        );
        let t1 = t.clone();
        del_all.base.enabled.set_fn(move |_| !t1().group_list.list.borrow().is_empty());
        let dar: ControlRef = del_all.clone();
        let t1 = t.clone();
        let new_btn = ButtonControl::new(
            anchor("RIGHT", &dar, "LEFT"),
            rect(-4.0, 0.0, 60.0, 18.0),
            "New".to_string(),
            move |g| {
                let s = t1();
                let i = s.edit(Engine::new_socket_group);
                s.sync_list();
                s.group_list.sel_index.set(Some(i));
                *s.group_list.sel_value.borrow_mut() = s.group_list.list.borrow().get(i - 1).cloned();
                s.set_display_group(g, Some(i));
                s.add_undo_state(false);
                s.build().build_flag.set(true);
                // `return skillsTab.gemSlots[1].nameSpec`: the new selControl of the tab
                let first: ControlRef = s.gem_slots.borrow()[0].name_spec.clone();
                *s.pending_focus.borrow_mut() = Some(first);
            },
        );
        group_list.host.add("delete", del);
        group_list.host.add("deleteAll", del_all);
        group_list.host.add("new", new_btn);
        host.add("groupList", group_list.clone());
        let mut tip_h = rect(0.0, 8.0, 0.0, 14.0);
        tip_h[3] = 14.0.into();
        host.add(
            "groupTip",
            LabelControl::new(anchor("TOPLEFT", &glr, "BOTTOMLEFT"), tip_h, GROUP_TIP.to_string()),
        );

        // :125-154
        let (ox, oy) = (170.0, 45.0);
        let option_section: ControlRef = SectionControl::new(
            anchor("TOPLEFT", &glr, "BOTTOMLEFT"),
            rect(0.0, oy + 50.0, 360.0, 156.0),
            "Gem Options".to_string(),
        );
        host.add("optionSection", option_section.clone());
        let t1 = t.clone();
        let sort_gems_by_dps = CheckBoxControl::new(
            g,
            anchor("TOPLEFT", &glr, "BOTTOMLEFT"),
            [ox, oy + 70.0, 20.0],
            Some("Sort gems by DPS:"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                t1().edit(|e| {
                    e.set_sort_gems_by_dps(state);
                    Ok(())
                });
            })),
            None,
            true,
        );
        host.add("sortGemsByDPS", sort_gems_by_dps.clone());
        let sdr: ControlRef = sort_gems_by_dps.clone();
        let t1 = t.clone();
        let sort_field = DropDownControl::new(
            g,
            anchor("LEFT", &sdr, "RIGHT"),
            rect(10.0, 0.0, 140.0, 20.0),
            SORT_TYPES.iter().map(|(l, v)| item(l, v.to_string())).collect(),
            Some(Rc::new(move |_g: &mut Gfx, _i, v: &DropItem| {
                let f: String = data_of(v);
                t1().edit(|e| {
                    e.set_sort_gems_by_dps_field(&f);
                    Ok(())
                });
            })),
            None,
            false,
        );
        host.add("sortGemsByDPSFieldControl", sort_field.clone());
        let t1 = t.clone();
        let default_level = DropDownControl::new(
            g,
            anchor("TOPLEFT", &glr, "BOTTOMLEFT"),
            rect(ox, oy + 94.0, 170.0, 20.0),
            DEFAULT_LEVELS.iter().map(|(l, _, v)| item(l, v.to_string())).collect(),
            Some(Rc::new(move |_g: &mut Gfx, _i, v: &DropItem| {
                let f: String = data_of(v);
                t1().edit(|e| e.set_default_gem_level(&f));
            })),
            None,
            false,
        );
        default_level.tip.set_tooltip_func(
            |tooltip: &mut Tooltip, g: &Gfx, (mode, _i, v): (&'static str, usize, Option<DropItem>)| {
                tooltip.clear(false);
                if mode != "OUT"
                    && let Some(v) = v
                {
                    let id: String = data_of(&v);
                    if let Some(d) = DEFAULT_LEVELS.iter().find(|x| x.2 == id) {
                        tooltip.add_line(g, 16.0, Some(&format!("^7{}", d.1)));
                    }
                }
            },
        );
        host.add("defaultLevel", default_level.clone());
        let dlr: ControlRef = default_level.clone();
        host.add(
            "defaultLevelLabel",
            LabelControl::new(
                anchor("RIGHT", &dlr, "LEFT"),
                rect(-4.0, 0.0, 0.0, 16.0),
                "^7Default gem level:".to_string(),
            ),
        );
        let t1 = t.clone();
        let default_quality_c = EditControl::new(
            g,
            anchor("TOPLEFT", &glr, "BOTTOMLEFT"),
            rect(ox, oy + 118.0, 60.0, 20.0),
            EditArgs {
                filter: Some("%D".into()),
                limit: Some(2),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    let v = buf.parse::<f64>().ok();
                    t1().edit(|e| e.set_default_gem_quality(v));
                })),
                ..Default::default()
            },
        );
        host.add("defaultQuality", default_quality_c.clone());
        let dqr: ControlRef = default_quality_c.clone();
        host.add(
            "defaultQualityLabel",
            LabelControl::new(
                anchor("RIGHT", &dqr, "LEFT"),
                rect(-4.0, 0.0, 0.0, 16.0),
                "^7Default gem quality:".to_string(),
            ),
        );
        let t1 = t.clone();
        let show_support = DropDownControl::new(
            g,
            anchor("TOPLEFT", &glr, "BOTTOMLEFT"),
            rect(ox, oy + 142.0, 170.0, 20.0),
            SHOW_SUPPORT.iter().map(|(l, v)| item(l, v.to_string())).collect(),
            Some(Rc::new(move |_g: &mut Gfx, _i, v: &DropItem| {
                let f: String = data_of(v);
                t1().edit(|e| {
                    e.set_show_support_gem_types(&f);
                    Ok(())
                });
            })),
            None,
            false,
        );
        host.add("showSupportGemTypes", show_support.clone());
        let ssr: ControlRef = show_support.clone();
        host.add(
            "showSupportGemTypesLabel",
            LabelControl::new(
                anchor("RIGHT", &ssr, "LEFT"),
                rect(-4.0, 0.0, 0.0, 16.0),
                "^7Show support gems:".to_string(),
            ),
        );
        let t1 = t.clone();
        let show_legacy = CheckBoxControl::new(
            g,
            anchor("TOPLEFT", &glr, "BOTTOMLEFT"),
            [ox, oy + 166.0, 20.0],
            Some("^7Show legacy gems:"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                t1().edit(|e| {
                    e.set_show_legacy_gems(state);
                    Ok(())
                });
            })),
            None,
            false,
        );
        host.add("showLegacyGems", show_legacy.clone());

        // :157-417
        let anchor_group_detail = BareControl::new(anchor("TOPLEFT", &glr, "TOPRIGHT"), rect(20.0, 0.0, 0.0, 0.0));
        let t1 = t.clone();
        anchor_group_detail
            .base
            .shown
            .set_fn(move |_| t1().display_group.borrow().is_some());
        let agd: ControlRef = anchor_group_detail.clone();
        let (t1, f1) = (t.clone(), flag.clone());
        let group_label = EditControl::new(
            g,
            anchor("TOPLEFT", &agd, "TOPLEFT"),
            rect(0.0, 0.0, 380.0, 20.0),
            EditArgs {
                prompt: Some("Label".into()),
                filter: Some("%c".into()),
                limit: Some(50),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    let s = t1();
                    let d = s.display_index().expect("displayGroup");
                    s.edit(|e| e.set_group_label(d, buf));
                    f1();
                })),
                ..Default::default()
            },
        );
        host.add("groupLabel", group_label.clone());
        let group_slot_label = LabelControl::new(
            anchor("TOPLEFT", &agd, "TOPLEFT"),
            rect(0.0, 30.0, 0.0, 16.0),
            "^7Socketed in:".to_string(),
        );
        host.add("groupSlotLabel", group_slot_label.clone());
        let (t1, f1) = (t.clone(), flag.clone());
        let group_slot = DropDownControl::new(
            g,
            anchor("TOPLEFT", &agd, "TOPLEFT"),
            rect(85.0, 28.0, 130.0, 20.0),
            group_slot_list(),
            Some(Rc::new(move |g: &mut Gfx, _i, v: &DropItem| {
                let s = t1();
                let d = s.display_index().expect("displayGroup");
                let slot: Option<String> = data_of(v);
                if s.edit(|e| e.set_group_slot(d, slot.as_deref())) {
                    let im = s.imbued();
                    *im.gem_id.borrow_mut() = None;
                    im.edit.set_text(g, "", false);
                }
                f1();
            })),
            None,
            false,
        );
        let t1 = t.clone();
        group_slot.tip.set_tooltip_func(
            move |tooltip: &mut Tooltip, g: &Gfx, (mode, index, value): (&'static str, usize, Option<DropItem>)| {
                tooltip.clear(false);
                let slot: Option<String> = value.as_ref().map(data_of).unwrap_or(None);
                if mode == "OUT" || index == 1 {
                    tooltip.add_line(g, 16.0, Some("Select the item in which this skill is socketed."));
                    tooltip.add_line(
                        g,
                        16.0,
                        Some("This will allow the skill to benefit from modifiers on the item that affect socketed gems."),
                    );
                } else if let Some(slot) = slot {
                    if let Some(item) = t1().read(|e| e.slot_item(&slot)) {
                        let b = t1().build();
                        let tab = b.items_tab.borrow().clone().expect("itemsTab");
                        tab.add_slot_tooltip(tooltip, g, &item, &slot);
                    } else {
                        tooltip.add_line(g, 16.0, Some("No item is equipped in this slot."));
                    }
                }
            },
        );
        let t1 = t.clone();
        group_slot.base.enabled.set_fn(move |_| {
            let s = t1();
            s.display_index()
                .is_some_and(|d| s.read(|e| e.socket_groups()[d - 1].source.is_none()))
        });
        host.add("groupSlot", group_slot.clone());
        let gsr: ControlRef = group_slot.clone();
        let (t1, f1) = (t.clone(), flag.clone());
        let group_enabled = CheckBoxControl::new(
            g,
            anchor("LEFT", &gsr, "RIGHT"),
            [70.0, 0.0, 20.0],
            Some("Enabled:"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                let s = t1();
                let d = s.display_index().expect("displayGroup");
                s.edit(|e| e.set_group_enabled(d, state));
                f1();
            })),
            None,
            false,
        );
        host.add("groupEnabled", group_enabled.clone());
        let ger: ControlRef = group_enabled.clone();
        let (t1, f1) = (t.clone(), flag.clone());
        let include_in_full_dps = CheckBoxControl::new(
            g,
            anchor("LEFT", &ger, "RIGHT"),
            [145.0, 0.0, 20.0],
            Some("Include in Full DPS:"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                let s = t1();
                let d = s.display_index().expect("displayGroup");
                s.edit(|e| e.set_group_full_dps(d, state));
                f1();
            })),
            None,
            false,
        );
        host.add("includeInFullDPS", include_in_full_dps.clone());
        let gslr: ControlRef = group_slot_label.clone();
        let t1 = t.clone();
        let sockets_label = LabelControl::new(
            anchor("TOPLEFT", &gslr, "BOTTOMLEFT"),
            rect(0.0, 8.0, 0.0, 16.0),
            Prop::func(move |_| {
                let line = match t1().selected_item() {
                    Some((item, _)) => {
                        if item.base.is_some_and(|b| b.socket_limit.is_none()) {
                            "This item cannot have sockets".to_string()
                        } else {
                            socket_description_line(&item)
                        }
                    }
                    None => String::new(),
                };
                format!("^7Item sockets: {line}")
            }),
        );
        let t1 = t.clone();
        sockets_label.base.shown.set_fn(move |_| t1().selected_item().is_some());
        host.add("socketsLabel", sockets_label.clone());
        let slr: ControlRef = sockets_label.clone();
        let optimise_sockets = ButtonControl::new(
            anchor("LEFT", &slr, "RIGHT"),
            rect(4.0, 0.0, 120.0, 18.0),
            "Optimise Sockets".to_string(),
            {
                let t1 = t.clone();
                move |g| t1().optimise_sockets(g)
            },
        );
        let t1 = t.clone();
        optimise_sockets.base.shown.set_fn(move |_| {
            t1().selected_item()
                .is_some_and(|(i, _)| i.base.is_some_and(|b| b.socket_limit.is_some()))
        });
        let t1 = t.clone();
        optimise_sockets.base.enabled.set_fn(move |_| {
            let s = t1();
            let Some((item, slot)) = s.selected_item() else { return false };
            let Some(base) = item.base else { return false };
            let abyssal = item.sockets.iter().filter(|x| x.color == "A").count() as f64;
            let mut max = base.socket_limit.unwrap_or(0.0) - abyssal;
            s.read(|e| {
                let skills = pob_data::skills::skills();
                for group in e.socket_groups() {
                    if group.slot.as_deref() == Some(slot.as_str()) {
                        for gem in &group.gem_list {
                            let has = gem.granted_effect.as_deref().and_then(|i| skills.get(i)).is_some()
                                || gem.gem_data.as_deref().and_then(gem_of).is_some();
                            if has && max > 0.0 {
                                if gem.matches_socket != Some(true) {
                                    return true;
                                }
                                max -= 1.0;
                            }
                        }
                    }
                }
                false
            })
        });
        optimise_sockets
            .tip
            .tooltip_text
            .set(Some("Rebuild the item's sockets to match the groups assigned to it.".to_string()));
        host.add("optimiseSockets", optimise_sockets.clone());
        let mut a = anchor("TOPLEFT", &slr, "BOTTOMLEFT");
        if let Some(x) = a.as_mut() {
            x.collapse = true;
        }
        let imbued_label = LabelControl::new(a, rect(0.0, 8.0, 0.0, 16.0), format!("{}Imbued Support:", cc("CRAFTED")));
        let t1 = t.clone();
        imbued_label.base.shown.set_fn(move |_| {
            let s = t1();
            s.display_index()
                .is_some_and(|d| s.read(|e| e.socket_groups()[d - 1].source.is_none()))
        });
        host.add("imbuedSupportLabel", imbued_label.clone());
        let ilr: ControlRef = imbued_label.clone();
        let is_enabled = {
            let t1 = t.clone();
            move |_: &Gfx| {
                let s = t1();
                s.display_index().is_some_and(|d| {
                    s.read(|e| {
                        let x = &e.socket_groups()[d - 1];
                        let Some(slot) = &x.slot else { return false };
                        let has = e.skills_tab().imbued_support_by_slot.contains_key(slot);
                        (has && x.imbued_support.is_some()) || !has
                    })
                })
            }
        };
        let t1 = t.clone();
        let imbued_clear = ButtonControl::new(
            anchor("LEFT", &ilr, "RIGHT"),
            rect(260.0, 0.0, 20.0, 20.0),
            "x".to_string(),
            move |g| {
                let s = t1();
                let im = s.imbued();
                *im.gem_id.borrow_mut() = None;
                im.edit.set_text(g, "", false);
                let f = im.gem_change_func.clone();
                f(g, None, false, false, false);
            },
        );
        imbued_clear.base.enabled.set_fn(is_enabled.clone());
        imbued_clear.tip.tooltip_text.set(Some("Remove this imbued support.".to_string()));
        host.add("imbuedSupportClear", imbued_clear.clone());
        let ifd: ControlRef = include_in_full_dps.clone();
        let group_count_label = LabelControl::new(anchor("LEFT", &ifd, "RIGHT"), rect(16.0, 0.0, 0.0, 16.0), "^7Count:".to_string());
        let has_source = {
            let t1 = t.clone();
            move |_: &Gfx| {
                let s = t1();
                s.display_index()
                    .is_some_and(|d| s.read(|e| e.socket_groups()[d - 1].source.is_some()))
            }
        };
        group_count_label.base.shown.set_fn(has_source.clone());
        host.add("groupCountLabel", group_count_label.clone());
        let gclr: ControlRef = group_count_label.clone();
        let (t1, f1) = (t.clone(), flag.clone());
        let group_count = EditControl::new(
            g,
            anchor("LEFT", &gclr, "RIGHT"),
            rect(4.0, 0.0, 60.0, 20.0),
            EditArgs {
                filter: Some("%D".into()),
                limit: Some(2),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    let s = t1();
                    let d = s.display_index().expect("displayGroup");
                    let v = buf.parse::<f64>().ok();
                    s.edit(|e| e.set_group_count(d, v));
                    f1();
                })),
                ..Default::default()
            },
        );
        group_count.base.shown.set_fn(has_source.clone());
        host.add("groupCount", group_count.clone());
        let t1 = t.clone();
        let source_note = LabelControl::new(
            anchor("TOPLEFT", &slr, "TOPLEFT"),
            rect(0.0, 30.0, 0.0, 16.0),
            Prop::func(move |_| t1().source_note_text()),
        );
        source_note.base.shown.set_fn(has_source);
        host.add("sourceNote", source_note.clone());
        let scroll_bar_h = ScrollBarControl::new(None, rect(0.0, 0.0, 0.0, 18.0), Some(100.0), Some("HORIZONTAL"), true);
        host.add("scrollBarH", scroll_bar_h.clone());
        let anchor_gem_slots = BareControl::new(anchor("TOPLEFT", &ilr, "BOTTOMLEFT"), rect(0.0, 30.0, 0.0, 0.0));
        SkillsTab {
            host,
            me: me.clone(),
            build: build.clone(),
            undo: UndoHandler::default(),
            anchor: anchor_c,
            set_select,
            group_list,
            sort_gems_by_dps,
            sort_field,
            default_level,
            default_quality: default_quality_c,
            show_support,
            show_legacy,
            group_label,
            group_slot,
            group_enabled,
            include_in_full_dps,
            sockets_label,
            optimise_sockets,
            imbued_label,
            imbued_support: RefCell::new(None),
            imbued_clear,
            group_count,
            group_count_label,
            source_note,
            scroll_bar_h,
            anchor_group_detail,
            anchor_gem_slots,
            gem_slots: RefCell::new(Vec::new()),
            display_group: RefCell::new(None),
            pending_focus: RefCell::new(None),
            set_list: RefCell::new(Weak::new()),
            icons: RefCell::new(HashMap::new()),
        }
    });
    // :322-369 the control needs the tab to exist: `PopulateGemList` reads it
    let w = Rc::downgrade(&tab);
    let change: GemChangeFunc = Rc::new(move |g: &mut Gfx, gem_id: Option<String>, _add, _lost, gem_match| {
        let s = w.upgrade().expect("skillsTab");
        let d = s.display_index();
        let Some(target) = d.and_then(|d| s.read(|e| e.socket_groups()[d - 1].slot.clone())) else {
            return;
        };
        s.add_undo_state(false);
        let id = if gem_match { gem_id } else { None };
        let (name, build_flag) = s.edit(|e| e.set_imbued_support(&target, d, id.as_deref()));
        if let (Some(_), Some(id)) = (name, id) {
            let skills = pob_data::skills::skills();
            let col = gem_of(&id).and_then(|x| skills.get(&x.granted_effect_id)).and_then(|x| x.color);
            *s.imbued().edit.inactive_col.borrow_mut() = skill_color(col);
        }
        if build_flag {
            s.build().build_flag.set(true);
        }
        let _ = g;
    });
    let ilr: ControlRef = tab.imbued_label.clone();
    let imbued = GemSelectControl::new(
        g,
        anchor("LEFT", &ilr, "RIGHT"),
        rect(8.0, 0.0, 250.0, 20.0),
        Rc::downgrade(&tab),
        1,
        change,
        true,
        true,
    );
    let w = Rc::downgrade(&tab);
    imbued.base().enabled.set_fn(move |_| {
        let s = w.upgrade().expect("skillsTab");
        s.display_index().is_some_and(|d| {
            s.read(|e| {
                let x = &e.socket_groups()[d - 1];
                let Some(slot) = &x.slot else { return false };
                let has = e.skills_tab().imbued_support_by_slot.contains_key(slot);
                (has && x.imbued_support.is_some()) || !has
            })
        })
    });
    tab.host.add("imbuedSupport", imbued.clone());
    *tab.imbued_support.borrow_mut() = Some(imbued);
    // SkillsTab:Load (:523-571)
    sel_by_value(&tab.default_level, &default_gem_level);
    tab.default_quality.set_text(g, &tostring_num(default_quality), false);
    if let Some(s) = opts.sort_gems_by_dps {
        tab.sort_gems_by_dps.state.set(s);
    }
    if let Some(s) = opts.show_legacy_gems {
        tab.show_legacy.state.set(s);
    }
    sel_by_value(&tab.show_support, &opts.show_support_gem_types);
    sel_by_value(&tab.sort_field, &opts.sort_gems_by_dps_field);
    tab.create_gem_slot(g, 1);
    {
        let s1 = tab.gem_slots.borrow()[0].clone();
        let name: ControlRef = s1.name_spec.clone();
        let level: ControlRef = s1.level.clone();
        let quality: ControlRef = s1.quality.clone();
        let enabled: ControlRef = s1.enabled.clone();
        let count: ControlRef = s1.count.clone();
        tab.host.add(
            "gemNameHeader",
            LabelControl::new(
                anchor("BOTTOMLEFT", &name, "TOPLEFT"),
                rect(0.0, -2.0, 0.0, 16.0),
                "^7Gem name:".to_string(),
            ),
        );
        tab.host.add(
            "gemLevelHeader",
            LabelControl::new(
                anchor("BOTTOMLEFT", &level, "TOPLEFT"),
                rect(0.0, -2.0, 0.0, 16.0),
                "^7Level:".to_string(),
            ),
        );
        tab.host.add(
            "gemQualityHeader",
            LabelControl::new(
                anchor("BOTTOMLEFT", &quality, "TOPLEFT"),
                rect(0.0, -2.0, 0.0, 16.0),
                "^7Quality:".to_string(),
            ),
        );
        tab.host.add(
            "gemEnableHeader",
            LabelControl::new(
                anchor("BOTTOMLEFT", &enabled, "TOPLEFT"),
                rect(-16.0, -2.0, 0.0, 16.0),
                "^7Enabled:".to_string(),
            ),
        );
        tab.host.add(
            "gemCountHeader",
            LabelControl::new(
                anchor("BOTTOMLEFT", &count, "TOPLEFT"),
                rect(8.0, -2.0, 0.0, 16.0),
                "^7Count:".to_string(),
            ),
        );
    }
    // `SetActiveSkillSet(xml.attrib.activeSkillSet)` of `Load`: the groups of the XML, before CalcSetup
    tab.sync_list();
    let has_first = tab.read(|e| !pob_calc::pob_build::active_skill_groups(e.model()).is_empty());
    tab.set_display_group(g, has_first.then_some(1));
    let st = tab.read(Engine::skills_load_undo_state).unwrap_or_else(|e| panic!("{e}"));
    tab.undo.undo.borrow_mut().clear();
    tab.undo.redo.borrow_mut().clear();
    tab.undo.undo.borrow_mut().push(Rc::new(st));
    tab
}

/// The state of the real window (skills-rust-input.ps1): the same fields as `RealState` of skills-runner.lua. `null` until the tab exists.
pub fn debug_input_state(g: &Gfx) -> serde_json::Value {
    use serde_json::json;
    let b = main().build_mode.clone();
    if main().mode.borrow().as_deref() != Some("BUILD") {
        return serde_json::Value::Null;
    }
    let Some(t) = b.skills_tab.borrow().clone() else {
        return serde_json::Value::Null;
    };
    let pos = |c: &dyn Control| {
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        json!({ "x": (x + w / 2.0).floor(), "y": (y + h / 2.0).floor() })
    };
    let (lx, ly) = t.group_list.get_pos(g);
    let c = b.c();
    let stat: Vec<String> = c
        .stat_box
        .list
        .borrow()
        .iter()
        .map(|l| {
            let col = |i: usize| l.cols.get(i).cloned().flatten().unwrap_or("nil".into());
            format!("{}|{}", col(0), col(1))
        })
        .collect();
    let slot = t.gem_slots.borrow()[0].clone();
    let mode = b.host.get("modeSkills").expect("modeSkills");
    let gem_quality = t.read(|e| e.socket_groups().get(3).and_then(|x| x.gem_list.first()).and_then(|x| x.quality));
    json!({
        "viewMode": *b.view_mode.borrow(), "modeBtn": pos(&*mode), "rowPos": { "x": lx + 10.0, "y": ly + 2.0 + 16.0 * 3.0 + 8.0 }, "qualityPos": pos(&*slot.quality),
        "display": t.display_index().unwrap_or(0), "selIndex": t.group_list.sel_index.get().unwrap_or(0), "label": t.group_label.buf.borrow().clone(),
        "name": slot.name_spec.edit.buf.borrow().clone(), "quality": slot.quality.buf.borrow().clone(), "gemQuality": gem_quality,
        "scrollH": t.scroll_bar_h.offset.get(), "statBox": stat,
    })
}

/// `ItemsTab:GetSocketDescriptionLine(item)` (ItemsTab.lua:4245-4273).
fn socket_description_line(item: &pob_items::Item) -> String {
    let mut group = 0.0;
    let mut line = String::new();
    for (i, s) in item.sockets.iter().enumerate() {
        if i > 0 {
            if s.group == group {
                line.push_str("^7=");
            } else {
                line.push_str("  ");
            }
            group = s.group;
        }
        let code = match s.color.as_str() {
            "R" => cc("STRENGTH"),
            "G" => cc("DEXTERITY"),
            "B" => cc("INTELLIGENCE"),
            "W" => cc("SCION"),
            "A" => "^xB0B0B0".to_string(),
            o => panic!("Classes/ItemsTab.lua:4270: attempt to concatenate local 'code' (a nil value): socket {o}"),
        };
        line.push_str(&code);
        line.push_str(&s.color);
    }
    line
}
