//! `Classes/CompareEntry.lua`: a build next to the primary one: its own engine, no sidebar, no `RefreshStatList` in the Lua.

use std::rc::Rc;

use pob_calc::pob_build::InputValue;
use pob_gfx::Gfx;

use crate::modules::build::BuildMode;

pub struct CompareEntry {
    /// `self.label`.
    pub label: String,
    /// The build object of the entry: the engine (`calcsTab`, `itemsTab`, `skillsTab`, `treeTab`, `spec`) and `configTab`.
    pub b: Rc<BuildMode>,
    /// The primary build: `GlobalCache` is shared with it.
    pub primary: std::rc::Weak<BuildMode>,
}

/// What `CompareEntry:LoadFromXML` reads differently from `Build:Init`, as the XML the engine loads:
/// - `<Party>` has no saver (:119-129, `self.partyTab` is a stub with an empty list and actor);
/// - nothing runs `EstimatePlayerProgress` (Build.lua:910): `characterLevel` stays the XML's, also in auto mode.
fn entry_xml(xml: &str) -> String {
    // `exportParty`: `self.partyTab` is that stub, no flag on it (the Link branch of CalcPerform.lua:2737 reads it)
    let mut s = xml
        .replace("characterLevelAutoMode=\"true\"", "characterLevelAutoMode=\"false\"")
        .replace("exportParty=\"true\"", "exportParty=\"false\"");
    if let Some(a) = s.find("<Party") {
        let rest = &s[a..];
        let end = match (rest.find("/>"), rest.find('>'), rest.find("</Party>")) {
            (Some(c), Some(o), _) if c + 1 == o => Some(c + 2),
            (_, _, Some(e)) => Some(e + "</Party>".len()),
            _ => None,
        };
        if let Some(e) = end {
            s.replace_range(a..a + e, "");
        }
    }
    s
}

impl CompareEntry {
    /// `CompareEntry:CompareEntry(xmlText, label)` + `LoadFromXML` (:15-186).
    pub fn new(g: &mut Gfx, xml: &str, label: &str, primary: std::rc::Weak<BuildMode>) -> Result<Rc<Self>, String> {
        let b = BuildMode::new();
        b.init_compare_entry(g, &entry_xml(xml), label)?;
        // :180 SyncCalcsSkillSelection (:229-248): `displaySkillList` is still nil here (CalcSetup sets it in the run below), only the group's fields are copied
        {
            let mut eng = b.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            let group = e.model().main_socket_group;
            let mut changed = !matches!(e.calcs_input().get("skill_number"), Some(InputValue::Num(n)) if *n == group);
            e.set_calcs_input("skill_number", Some(InputValue::Num(group)));
            let main_active = e
                .socket_groups()
                .get((group as usize).wrapping_sub(1))
                .map(|g| (g.main_active_skill, g.main_active_skill_calcs));
            if let Some((Some(a), calcs)) = main_active
                && calcs != Some(a)
            {
                e.set_main_active_skill_calcs(a)?;
                changed = true;
            }
            if changed {
                e.recalc()?;
            }
        }
        b.build_flag.set(false);
        Ok(Rc::new(Self {
            label: label.to_string(),
            b,
            primary,
        }))
    }

    /// `CompareEntry:Rebuild()` (:250-256): the engine's `refresh` without the auto level, which `entry_xml` turned off.
    pub fn rebuild(&self, g: &mut Gfx) {
        // `wipeGlobalCache()` (:251): the cache is one for all the builds, what the calculators of the others run over next is empty
        if let Some(p) = self.primary.upgrade()
            && let Some(e) = p.engine.borrow().as_ref()
        {
            e.wipe_misc_cache();
        }
        self.b
            .engine
            .borrow_mut()
            .as_mut()
            .expect("engine")
            .refresh()
            .unwrap_or_else(|e| panic!("{e}"));
        self.b.build_flag.set(false);
        let config = self.b.config_tab.borrow().clone();
        if let Some(t) = config {
            t.after_recalc(g);
        }
    }

    /// `SetActiveSpec(index)` (:258-263).
    pub fn set_active_spec(&self, g: &mut Gfx, index: usize) {
        {
            let mut eng = self.b.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            if index < 1 || index > e.spec_list_labels().len() {
                return;
            }
            e.set_active_spec(index).unwrap_or_else(|err| panic!("{err}"));
            crate::tree_view::set_jewel_radii_globally(&e.spec_rc().tree_version);
        }
        let tab = self.b.items_tab.borrow().clone().expect("itemsTab");
        tab.populate_slots(g);
        self.rebuild(g);
    }

    /// `entry.treeTab:ConvertToVersion(version, remove, false, true)` (TreeTab.lua:593-617).
    pub fn convert_to_version(&self, g: &mut Gfx, version: &str, remove: bool) {
        {
            let mut eng = self.b.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            e.convert_to_version(version, remove, true).unwrap_or_else(|err| panic!("{err}"));
            crate::tree_view::set_jewel_radii_globally(&e.spec_rc().tree_version);
        }
        let tab = self.b.items_tab.borrow().clone().expect("itemsTab");
        tab.populate_slots(g);
        self.b.build_flag.set(true);
    }

    /// :317-319: `entry:SetActiveSkillSet(skillSetOrderList[index])`.
    pub fn set_active_skill_set_index(&self, g: &mut Gfx, index: usize) {
        {
            let mut eng = self.b.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            let Some((id, _)) = e.skill_sets().get(index.wrapping_sub(1)).cloned() else {
                return;
            };
            e.set_active_skill_set(id).unwrap_or_else(|err| panic!("{err}"));
        }
        self.rebuild(g);
    }

    /// :327-329: `entry:SetActiveItemSet(itemSetOrderList[index])`.
    pub fn set_active_item_set_index(&self, g: &mut Gfx, index: usize) {
        let id = {
            let eng = self.b.engine.borrow();
            eng.as_ref().expect("engine").item_sets().get(index.wrapping_sub(1)).map(|s| s.id)
        };
        let Some(id) = id else { return };
        let tab = self.b.items_tab.borrow().clone().expect("itemsTab");
        tab.set_active_item_set(g, id);
        self.rebuild(g);
    }

    /// :337-343: `configTab:SetActiveConfigSet(setId)`, `buildFlag = true`.
    pub fn set_active_config_set_index(&self, g: &mut Gfx, index: usize) -> bool {
        let id = {
            let eng = self.b.engine.borrow();
            eng.as_ref().expect("engine").config_set_order().get(index.wrapping_sub(1)).copied()
        };
        let Some(id) = id else { return false };
        let tab = self.b.config_tab.borrow().clone().expect("configTab");
        tab.set_active_config_set(g, id);
        self.b.build_flag.set(true);
        true
    }
}
